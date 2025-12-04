use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("No sessions found")]
    NoSessions,
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("Server error: {0}")]
    Server(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub pid: u32,
    pub started: String,
    pub command: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request {
    GetScrollback { lines: Option<usize> },
    GetCursor,
    Inject { data: String },
    GetSize,
    Subscribe,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Response {
    Scrollback { content: String },
    Cursor { row: usize, col: usize },
    Size { rows: u16, cols: u16 },
    Output { data: Vec<u8> },
    Subscribed,
    Ok,
    Error { message: String },
}

fn get_socket_dir() -> PathBuf {
    dirs::runtime_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".record")))
        .unwrap_or_else(|| PathBuf::from("/tmp/record"))
}

/// List all active record sessions
pub fn list_sessions() -> Result<Vec<Session>> {
    let sessions_file = get_socket_dir().join("sessions.json");
    let content = std::fs::read_to_string(&sessions_file).unwrap_or_else(|_| "[]".to_string());
    let sessions: Vec<Session> = serde_json::from_str(&content)?;

    // Filter to only sessions with valid sockets
    let sessions: Vec<Session> = sessions
        .into_iter()
        .filter(|s| get_socket_dir().join(format!("{}.sock", s.id)).exists())
        .collect();

    Ok(sessions)
}

/// Client for interacting with a record session
pub struct Client {
    stream: BufReader<UnixStream>,
}

impl Client {
    /// Connect to a session by ID
    pub async fn connect(session_id: &str) -> Result<Self> {
        let socket_path = get_socket_dir().join(format!("{session_id}.sock"));
        if !socket_path.exists() {
            return Err(Error::SessionNotFound(session_id.to_string()));
        }
        let stream = UnixStream::connect(&socket_path).await?;
        Ok(Self {
            stream: BufReader::new(stream),
        })
    }

    /// Connect to the most recent session
    pub async fn connect_latest() -> Result<Self> {
        let sessions = list_sessions()?;
        let session = sessions.last().ok_or(Error::NoSessions)?;
        Self::connect(&session.id).await
    }

    async fn send_request(&mut self, request: &Request) -> Result<Response> {
        let request_bytes = serde_json::to_vec(request)?;
        self.stream.get_mut().write_all(&request_bytes).await?;

        let mut line = String::new();
        self.stream.read_line(&mut line).await?;
        let response: Response = serde_json::from_str(&line)?;
        Ok(response)
    }

    /// Get scrollback buffer content
    pub async fn get_scrollback(&mut self, lines: Option<usize>) -> Result<String> {
        let response = self.send_request(&Request::GetScrollback { lines }).await?;
        match response {
            Response::Scrollback { content } => Ok(content),
            Response::Error { message } => Err(Error::Server(message)),
            _ => Err(Error::Server("Unexpected response".to_string())),
        }
    }

    /// Get cursor position (row, col)
    pub async fn get_cursor(&mut self) -> Result<(usize, usize)> {
        let response = self.send_request(&Request::GetCursor).await?;
        match response {
            Response::Cursor { row, col } => Ok((row, col)),
            Response::Error { message } => Err(Error::Server(message)),
            _ => Err(Error::Server("Unexpected response".to_string())),
        }
    }

    /// Get terminal size (rows, cols)
    pub async fn get_size(&mut self) -> Result<(u16, u16)> {
        let response = self.send_request(&Request::GetSize).await?;
        match response {
            Response::Size { rows, cols } => Ok((rows, cols)),
            Response::Error { message } => Err(Error::Server(message)),
            _ => Err(Error::Server("Unexpected response".to_string())),
        }
    }

    /// Inject input into the PTY
    pub async fn inject(&mut self, data: &str) -> Result<()> {
        let response = self
            .send_request(&Request::Inject {
                data: data.to_string(),
            })
            .await?;
        match response {
            Response::Ok => Ok(()),
            Response::Error { message } => Err(Error::Server(message)),
            _ => Err(Error::Server("Unexpected response".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_dir() {
        let dir = get_socket_dir();
        assert!(!dir.as_os_str().is_empty());
    }

    #[test]
    fn test_list_sessions_empty() {
        // This should not panic even if no sessions exist
        let result = list_sessions();
        assert!(result.is_ok());
    }
}
