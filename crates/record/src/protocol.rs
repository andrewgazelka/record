use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Get the last N lines from scrollback buffer
    GetScrollback { lines: Option<usize> },
    /// Get current cursor position
    GetCursor,
    /// Inject input into the PTY
    Inject { data: String },
    /// Get terminal size
    GetSize,
    /// Subscribe to live output
    Subscribe,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Scrollback buffer content
    Scrollback { content: String },
    /// Cursor position
    Cursor { row: usize, col: usize },
    /// Terminal size
    Size { rows: u16, cols: u16 },
    /// Live output data (for subscribed clients)
    Output { data: Vec<u8> },
    /// Subscription confirmed
    Subscribed,
    /// Success
    Ok,
    /// Error
    Error { message: String },
}
