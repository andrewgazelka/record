---
description: Monitor and interact with other terminal windows running tap sessions. Use when you need to see what's happening in another terminal, check server output, or inject commands into a tapped session.
---

# tap - Terminal Introspection and Control

`tap` lets you see and control other terminal windows. When a user runs `tap` in another terminal, you can monitor its output, check cursor position, and even inject commands.

## Prerequisites

The user must have `tap` running in another terminal:
```sh
tap                  # Start tapping with default shell
tap start htop       # Start tapping with specific command
```

## Available Commands

### List Active Sessions

```sh
tap list
```

Shows all active tap sessions with their human-readable IDs (e.g., `blue-moon-fire`), PIDs, start times, and commands.

### Read Terminal Output

```sh
tap scrollback                    # Get full scrollback from latest session
tap scrollback -l 50              # Get last 50 lines
tap scrollback -s blue-moon-fire  # From specific session
```

Use this to see what's displayed in the other terminal - server logs, command output, error messages, etc.

### Get Cursor Position

```sh
tap cursor                        # From latest session
tap cursor -s blue-moon-fire      # From specific session
```

Returns `Row: N, Col: M` - useful for understanding where the user is in the terminal.

### Get Terminal Size

```sh
tap size                          # From latest session
tap size -s blue-moon-fire        # From specific session
```

Returns dimensions like `24x80` (rows x columns).

### Inject Input

```sh
tap inject "ls -la"               # Type into latest session
tap inject -s blue-moon-fire "cd /tmp"  # Into specific session
```

**Important**: This types the text but does NOT press Enter. To execute a command:
```sh
tap inject "ls -la\n"             # Include newline to execute
```

### Subscribe to Live Output

```sh
tap subscribe                     # Stream from latest session
tap subscribe -s blue-moon-fire   # From specific session
```

Streams live terminal output until interrupted. Useful for watching logs in real-time.

## Common Use Cases

### Check if a dev server is running
```sh
tap scrollback -l 20
# Look for "Server running on :3000" or similar
```

### Watch for errors in a build
```sh
tap scrollback | grep -i error
```

### Run a command in the other terminal
```sh
tap inject "npm test\n"
# Wait a moment, then check output
tap scrollback -l 30
```

### Monitor a long-running process
```sh
tap subscribe
# Ctrl+C to stop
```

## Session IDs

Sessions have human-readable IDs like `blue-moon-fire` instead of UUIDs. Use `-s` or `--session` to target a specific session when multiple are running.

## Tips

1. **Always check sessions first**: Run `tap list` to see what's available
2. **Use line limits**: `tap scrollback -l 50` is faster than full scrollback
3. **Include newlines for commands**: `tap inject "command\n"` to execute
4. **Check output after injecting**: Wait briefly, then `tap scrollback` to see results
