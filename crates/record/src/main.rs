use std::env;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::PathBuf;
use std::process::ExitCode;

use bytes::BytesMut;
use clap::Parser;
use nix::libc;
use nix::pty::{self, OpenptyResult, Winsize};
use nix::sys::signal::{self, SigHandler, Signal};
use nix::sys::termios::{self, SetArg, Termios};
use nix::unistd::{self, ForkResult, Pid};
use parking_lot::RwLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

mod protocol;
mod scrollback;

use protocol::{Request, Response};
use scrollback::ScrollbackBuffer;

/// PTY wrapper with Unix socket API for terminal introspection
#[derive(Parser)]
#[command(name = "record", about = "PTY wrapper with live API")]
struct Args {
    /// Command to run (defaults to $SHELL)
    #[arg(trailing_var_arg = true)]
    command: Vec<String>,
}

static SCROLLBACK: RwLock<ScrollbackBuffer> = RwLock::new(ScrollbackBuffer::new());
static MASTER_FD: std::sync::OnceLock<i32> = std::sync::OnceLock::new();

fn get_socket_dir() -> PathBuf {
    dirs::runtime_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".record")))
        .unwrap_or_else(|| PathBuf::from("/tmp/record"))
}

fn get_socket_path(session_id: &str) -> PathBuf {
    get_socket_dir().join(format!("{session_id}.sock"))
}

fn setup_terminal(fd: &OwnedFd) -> nix::Result<Termios> {
    let orig = termios::tcgetattr(fd)?;
    let mut raw = orig.clone();
    termios::cfmakeraw(&mut raw);
    termios::tcsetattr(fd, SetArg::TCSANOW, &raw)?;
    Ok(orig)
}

fn restore_terminal(fd: &OwnedFd, termios: &Termios) {
    let _ = termios::tcsetattr(fd, SetArg::TCSANOW, termios);
}

fn get_window_size() -> Winsize {
    let mut ws: Winsize = unsafe { std::mem::zeroed() };
    unsafe {
        libc::ioctl(libc::STDIN_FILENO, libc::TIOCGWINSZ, &mut ws);
    }
    ws
}

fn set_window_size(fd: i32, ws: &Winsize) {
    unsafe {
        libc::ioctl(fd, libc::TIOCSWINSZ, ws);
    }
}

extern "C" fn handle_sigwinch(_: libc::c_int) {
    if let Some(&master_fd) = MASTER_FD.get() {
        let ws = get_window_size();
        set_window_size(master_fd, &ws);
    }
}

async fn handle_client(mut stream: UnixStream, output_rx: broadcast::Receiver<Vec<u8>>) {
    let mut buf = BytesMut::with_capacity(4096);
    let mut output_rx = output_rx;

    loop {
        buf.clear();

        tokio::select! {
            result = stream.read_buf(&mut buf) => {
                match result {
                    Ok(0) => break,
                    Ok(_) => {
                        let request: Request = match serde_json::from_slice(&buf) {
                            Ok(r) => r,
                            Err(e) => {
                                warn!("Invalid request: {e}");
                                continue;
                            }
                        };

                        let response = match request {
                            Request::GetScrollback { lines } => {
                                let scrollback = SCROLLBACK.read();
                                let content = scrollback.get_lines(lines);
                                Response::Scrollback { content }
                            }
                            Request::GetCursor => {
                                let scrollback = SCROLLBACK.read();
                                let (row, col) = scrollback.cursor_position();
                                Response::Cursor { row, col }
                            }
                            Request::Inject { data } => {
                                if let Some(&master_fd) = MASTER_FD.get() {
                                    let fd = unsafe { OwnedFd::from_raw_fd(master_fd) };
                                    let result = unistd::write(&fd, data.as_bytes());
                                    std::mem::forget(fd);
                                    match result {
                                        Ok(_) => Response::Ok,
                                        Err(e) => Response::Error { message: e.to_string() },
                                    }
                                } else {
                                    Response::Error { message: "No master FD".to_string() }
                                }
                            }
                            Request::GetSize => {
                                let ws = get_window_size();
                                Response::Size {
                                    rows: ws.ws_row,
                                    cols: ws.ws_col,
                                }
                            }
                            Request::Subscribe => {
                                Response::Subscribed
                            }
                        };

                        let response_bytes = serde_json::to_vec(&response).unwrap();
                        if stream.write_all(&response_bytes).await.is_err() {
                            break;
                        }
                        if stream.write_all(b"\n").await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Read error: {e}");
                        break;
                    }
                }
            }
            result = output_rx.recv() => {
                match result {
                    Ok(data) => {
                        let response = Response::Output { data };
                        let response_bytes = serde_json::to_vec(&response).unwrap();
                        if stream.write_all(&response_bytes).await.is_err() {
                            break;
                        }
                        if stream.write_all(b"\n").await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
}

async fn run_server(
    socket_path: PathBuf,
    output_tx: broadcast::Sender<Vec<u8>>,
) -> std::io::Result<()> {
    let _ = std::fs::remove_file(&socket_path);
    let std_listener = StdUnixListener::bind(&socket_path)?;
    std_listener.set_nonblocking(true)?;
    let listener = UnixListener::from_std(std_listener)?;

    info!("Listening on {}", socket_path.display());

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                debug!("Client connected");
                let output_rx = output_tx.subscribe();
                tokio::spawn(handle_client(stream, output_rx));
            }
            Err(e) => {
                error!("Accept error: {e}");
            }
        }
    }
}

fn wait_for_child(child: Pid) -> i32 {
    use nix::sys::wait::{waitpid, WaitStatus};
    loop {
        match waitpid(child, None) {
            Ok(WaitStatus::Exited(_, code)) => return code,
            Ok(WaitStatus::Signaled(_, sig, _)) => return 128 + sig as i32,
            Ok(_) => continue,
            Err(nix::errno::Errno::EINTR) => continue,
            Err(_) => return 1,
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    let session_id = uuid::Uuid::new_v4().to_string();
    let socket_dir = get_socket_dir();
    std::fs::create_dir_all(&socket_dir).expect("Failed to create socket directory");
    let socket_path = get_socket_path(&session_id);

    // Write session info
    let sessions_file = socket_dir.join("sessions.json");
    let mut sessions: Vec<serde_json::Value> = std::fs::read_to_string(&sessions_file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    sessions.push(serde_json::json!({
        "id": session_id,
        "pid": std::process::id(),
        "started": chrono::Utc::now().to_rfc3339(),
        "command": if args.command.is_empty() {
            vec![env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())]
        } else {
            args.command.clone()
        },
    }));
    std::fs::write(&sessions_file, serde_json::to_string_pretty(&sessions).unwrap())
        .expect("Failed to write sessions file");

    // Open PTY using openpty
    let ws = get_window_size();
    let OpenptyResult { master, slave } = pty::openpty(Some(&ws), None).expect("openpty failed");

    let master_raw_fd = master.as_raw_fd();

    // Store master FD for signal handler
    MASTER_FD.set(master_raw_fd).unwrap();

    // Set up SIGWINCH handler
    unsafe {
        signal::signal(Signal::SIGWINCH, SigHandler::Handler(handle_sigwinch))
            .expect("Failed to set SIGWINCH handler");
    }

    // Fork child process
    let child_pid = match unsafe { unistd::fork() } {
        Ok(ForkResult::Child) => {
            drop(master);

            unistd::setsid().expect("setsid failed");

            // Set controlling terminal
            unsafe {
                libc::ioctl(slave.as_raw_fd(), libc::TIOCSCTTY as _, 0);
            }

            // Dup slave to stdin/stdout/stderr using libc directly
            let slave_raw = slave.as_raw_fd();
            unsafe {
                libc::dup2(slave_raw, libc::STDIN_FILENO);
                libc::dup2(slave_raw, libc::STDOUT_FILENO);
                libc::dup2(slave_raw, libc::STDERR_FILENO);
            }

            if slave_raw > 2 {
                drop(slave);
            }

            let cmd = if args.command.is_empty() {
                vec![env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())]
            } else {
                args.command.clone()
            };

            let c_cmd: Vec<std::ffi::CString> = cmd
                .iter()
                .map(|s| std::ffi::CString::new(s.as_str()).unwrap())
                .collect();

            unistd::execvp(&c_cmd[0], &c_cmd).expect("execvp failed");
            unreachable!()
        }
        Ok(ForkResult::Parent { child }) => child,
        Err(e) => {
            eprintln!("Fork failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Close slave in parent
    drop(slave);

    // Save terminal state and set raw mode
    let stdin_fd = unsafe { OwnedFd::from_raw_fd(libc::STDIN_FILENO) };
    let orig_termios = match setup_terminal(&stdin_fd) {
        Ok(t) => Some(t),
        Err(e) => {
            debug!("Not a terminal or failed to set raw mode: {e}");
            None
        }
    };
    // Don't close stdin
    std::mem::forget(stdin_fd);

    // Set up broadcast channel for output
    let (output_tx, _) = broadcast::channel::<Vec<u8>>(1024);

    // Start server
    let server_output_tx = output_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = run_server(socket_path.clone(), server_output_tx).await {
            error!("Server error: {e}");
        }
    });

    println!("\x1b[2m[record: session {session_id}]\x1b[0m");

    // Main I/O loop
    let mut master_file = tokio::fs::File::from_std(unsafe {
        std::fs::File::from_raw_fd(master.as_raw_fd())
    });
    // Prevent double-close
    std::mem::forget(master);

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    let mut master_buf = vec![0u8; 4096];
    let mut stdin_buf = vec![0u8; 4096];

    let exit_code = loop {
        tokio::select! {
            result = master_file.read(&mut master_buf) => {
                match result {
                    Ok(0) => break 0,
                    Ok(n) => {
                        let data = master_buf[..n].to_vec();

                        // Update scrollback
                        SCROLLBACK.write().push(&data);

                        // Broadcast to subscribers
                        let _ = output_tx.send(data.clone());

                        // Write to stdout
                        if stdout.write_all(&data).await.is_err() {
                            break 1;
                        }
                        let _ = stdout.flush().await;
                    }
                    Err(e) => {
                        debug!("Master read error: {e}");
                        break 0;
                    }
                }
            }
            result = stdin.read(&mut stdin_buf) => {
                match result {
                    Ok(0) => break 0,
                    Ok(n) => {
                        let fd = unsafe { OwnedFd::from_raw_fd(master_raw_fd) };
                        if unistd::write(&fd, &stdin_buf[..n]).is_err() {
                            std::mem::forget(fd);
                            break 1;
                        }
                        std::mem::forget(fd);
                    }
                    Err(e) => {
                        debug!("Stdin read error: {e}");
                        break 0;
                    }
                }
            }
        }
    };

    // Restore terminal
    if let Some(ref termios) = orig_termios {
        let stdin_fd = unsafe { OwnedFd::from_raw_fd(libc::STDIN_FILENO) };
        restore_terminal(&stdin_fd, termios);
        std::mem::forget(stdin_fd);
    }

    // Clean up socket and session entry
    let socket_path = get_socket_path(&session_id);
    let _ = std::fs::remove_file(&socket_path);

    // Remove session from sessions.json
    let sessions_file = socket_dir.join("sessions.json");
    if let Ok(content) = std::fs::read_to_string(&sessions_file) {
        if let Ok(mut sessions) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
            sessions.retain(|s| s.get("id").and_then(|v| v.as_str()) != Some(&session_id));
            let _ = std::fs::write(&sessions_file, serde_json::to_string_pretty(&sessions).unwrap());
        }
    }

    // Wait for child
    let final_code = wait_for_child(child_pid);

    if final_code == 0 && exit_code == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(final_code as u8)
    }
}
