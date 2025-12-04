
const DEFAULT_SCROLLBACK_LINES: usize = 10000;
const MAX_LINE_LENGTH: usize = 4096;

/// A simple scrollback buffer that stores terminal output
pub struct ScrollbackBuffer {
    lines: Vec<String>,
    current_line: String,
    cursor_row: usize,
    cursor_col: usize,
    max_lines: usize,
}

impl ScrollbackBuffer {
    pub const fn new() -> Self {
        Self {
            lines: Vec::new(),
            current_line: String::new(),
            cursor_row: 0,
            cursor_col: 0,
            max_lines: DEFAULT_SCROLLBACK_LINES,
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        // Simple parsing - just handle newlines and basic content
        // A full implementation would parse ANSI escape sequences
        for &byte in data {
            match byte {
                b'\n' => {
                    self.lines.push(std::mem::take(&mut self.current_line));
                    self.cursor_row += 1;
                    self.cursor_col = 0;

                    // Trim old lines if we exceed max
                    while self.lines.len() > self.max_lines {
                        self.lines.remove(0);
                        self.cursor_row = self.cursor_row.saturating_sub(1);
                    }
                }
                b'\r' => {
                    self.cursor_col = 0;
                }
                0x08 => {
                    // Backspace
                    if self.cursor_col > 0 {
                        self.cursor_col -= 1;
                        if self.cursor_col < self.current_line.len() {
                            self.current_line.remove(self.cursor_col);
                        }
                    }
                }
                0x1b => {
                    // Start of escape sequence - for now, just skip
                    // A full implementation would parse these
                }
                _ if byte >= 0x20 && byte < 0x7f => {
                    // Printable ASCII
                    if self.current_line.len() < MAX_LINE_LENGTH {
                        self.current_line.push(byte as char);
                        self.cursor_col += 1;
                    }
                }
                _ => {
                    // UTF-8 continuation bytes or other - try to append
                    if self.current_line.len() < MAX_LINE_LENGTH {
                        // For simplicity, store raw bytes as replacement char
                        // A proper implementation would handle UTF-8 properly
                    }
                }
            }
        }
    }

    pub fn get_lines(&self, count: Option<usize>) -> String {
        let count = count.unwrap_or(self.lines.len() + 1);
        let start = self.lines.len().saturating_sub(count);

        let mut result = self.lines[start..].join("\n");
        if !self.current_line.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&self.current_line);
        }
        result
    }

    pub fn cursor_position(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.lines.clear();
        self.current_line.clear();
        self.cursor_row = 0;
        self.cursor_col = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_simple_text() {
        let mut buf = ScrollbackBuffer::new();
        buf.push(b"hello world");
        assert_eq!(buf.get_lines(None), "hello world");
    }

    #[test]
    fn test_push_with_newlines() {
        let mut buf = ScrollbackBuffer::new();
        buf.push(b"line1\nline2\nline3");
        assert_eq!(buf.get_lines(None), "line1\nline2\nline3");
    }

    #[test]
    fn test_get_last_n_lines() {
        let mut buf = ScrollbackBuffer::new();
        buf.push(b"line1\nline2\nline3\nline4\n");
        assert_eq!(buf.get_lines(Some(2)), "line3\nline4");
    }

    #[test]
    fn test_cursor_position() {
        let mut buf = ScrollbackBuffer::new();
        buf.push(b"hello\nworld");
        let (row, col) = buf.cursor_position();
        assert_eq!(row, 1);
        assert_eq!(col, 5);
    }

    #[test]
    fn test_clear() {
        let mut buf = ScrollbackBuffer::new();
        buf.push(b"some content\n");
        buf.clear();
        assert_eq!(buf.get_lines(None), "");
        assert_eq!(buf.cursor_position(), (0, 0));
    }
}
