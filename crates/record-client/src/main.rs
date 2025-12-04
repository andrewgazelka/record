use clap::{Parser, Subcommand};
use record_client::{list_sessions, Client};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "record-client", about = "Client for record sessions")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List all active record sessions
    List,
    /// Get scrollback buffer from a session
    Scrollback {
        /// Session ID (uses latest if not specified)
        #[arg(short, long)]
        session: Option<String>,
        /// Number of lines to retrieve
        #[arg(short, long)]
        lines: Option<usize>,
    },
    /// Get cursor position
    Cursor {
        /// Session ID (uses latest if not specified)
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Get terminal size
    Size {
        /// Session ID (uses latest if not specified)
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Inject input into a session
    Inject {
        /// Session ID (uses latest if not specified)
        #[arg(short, long)]
        session: Option<String>,
        /// Text to inject
        text: String,
    },
}

async fn get_client(session: Option<String>) -> Result<Client, record_client::Error> {
    match session {
        Some(id) => Client::connect(&id).await,
        None => Client::connect_latest().await,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args = Args::parse();

    match args.command {
        Command::List => {
            let sessions = list_sessions()?;
            if sessions.is_empty() {
                println!("No active sessions");
            } else {
                println!("{:<38} {:<8} {:<25} {}", "ID", "PID", "STARTED", "COMMAND");
                for session in sessions {
                    println!(
                        "{:<38} {:<8} {:<25} {}",
                        session.id,
                        session.pid,
                        session.started,
                        session.command.join(" ")
                    );
                }
            }
        }
        Command::Scrollback { session, lines } => {
            let mut client = get_client(session).await?;
            let content = client.get_scrollback(lines).await?;
            print!("{content}");
        }
        Command::Cursor { session } => {
            let mut client = get_client(session).await?;
            let (row, col) = client.get_cursor().await?;
            println!("Row: {row}, Col: {col}");
        }
        Command::Size { session } => {
            let mut client = get_client(session).await?;
            let (rows, cols) = client.get_size().await?;
            println!("{rows}x{cols}");
        }
        Command::Inject { session, text } => {
            let mut client = get_client(session).await?;
            client.inject(&text).await?;
            println!("Injected");
        }
    }

    Ok(())
}
