//! Output rows that come from the running game during Play: who said it
//! (the server or the tester's client, tagged and coloured like Studio's
//! Output context labels) and, for an error, where.
//!
//! Roblox's `LogService.MessageOut` hands an error over as one message: the
//! error itself (`ServerScriptService.Main:2: boom`, the script's full name
//! and line in front) and then its stack, indented —
//!
//! ```text
//! ServerScriptService.Main:2: boom
//!     Stack Begin
//!     Script 'ServerScriptService.Main', Line 2 - function f
//!     Script 'ServerScriptService.Main', Line 5
//!     Stack End
//! ```
//!
//! [`parse`] splits that into the error text, its location and the stack's
//! frames, so the row reads as the error and each frame opens its script.

/// Where a game message was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Context {
    Server,
    Client,
}

impl Context {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Context::Server => "Server",
            Context::Client => "Client",
        }
    }
}

/// A script and a line in it, as the game names them: `path` is the
/// script's full name in the running game (`Players.Tester.PlayerScripts.Hud`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Location {
    pub(crate) path: String,
    pub(crate) line: u32,
}

impl Location {
    /// `Hud:4`: the script's own name and the line, as Studio prints it.
    pub(crate) fn short(&self) -> String {
        let name = self.path.rsplit('.').next().unwrap_or(&self.path);
        format!("{name}:{}", self.line)
    }
}

/// The game's structured `print` arguments in the shape the Output renders.
pub(crate) fn print_parts(parts: &[rbx_play::protocol::PrintedPart]) -> Vec<rbx_lua::PrintPart> {
    use rbx_play::protocol::{PrintedPart, PrintedTable, PrintedValue};

    fn table(t: &PrintedTable) -> rbx_lua::TableDump {
        rbx_lua::TableDump {
            address: t.address.clone(),
            entries: t
                .entries
                .iter()
                .map(|e| rbx_lua::TableEntry {
                    key: e.key.clone(),
                    value: match &e.value {
                        PrintedValue::Text(s) => rbx_lua::PrintValue::Text(s.clone()),
                        PrintedValue::Table(t) => rbx_lua::PrintValue::Table(table(t)),
                        PrintedValue::Reference { reference } => {
                            rbx_lua::PrintValue::Reference(reference.clone())
                        }
                    },
                })
                .collect(),
            more: t.more,
        }
    }

    parts
        .iter()
        .map(|part| match part {
            PrintedPart::Text(s) => rbx_lua::PrintPart::Text(s.clone()),
            PrintedPart::Table(t) => rbx_lua::PrintPart::Table(table(t)),
        })
        .collect()
}

/// One stack frame: its location and whatever followed it (` - function f`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) location: Location,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GameMessage {
    pub(crate) context: Context,
    /// The message without its stack.
    pub(crate) text: String,
    /// Where an error happened: the `path:line:` in front of its text, or
    /// else its first stack frame.
    pub(crate) location: Option<Location>,
    pub(crate) frames: Vec<Frame>,
}

/// `Script 'ServerScriptService.Main', Line 2 - function f`.
fn frame(line: &str) -> Option<Frame> {
    let rest = line.trim().strip_prefix("Script '")?;
    let (path, rest) = rest.split_once("', Line ")?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let number = rest[..digits].parse().ok()?;
    Some(Frame {
        location: Location {
            path: path.to_string(),
            line: number,
        },
        detail: rest[digits..].trim().to_string(),
    })
}

/// `ServerScriptService.Main:2: boom` → `ServerScriptService.Main`, 2.
fn leading_location(text: &str) -> Option<Location> {
    let (path, rest) = text.split_once(':')?;
    if path.is_empty() || path.contains(char::is_whitespace) {
        return None;
    }
    let (number, _) = rest.split_once(':')?;
    Some(Location {
        path: path.to_string(),
        line: number.parse().ok()?,
    })
}

pub(crate) fn parse(context: Context, message: &str) -> GameMessage {
    let mut text = Vec::new();
    let mut frames = Vec::new();
    let mut in_stack = false;
    for line in message.lines() {
        match line.trim() {
            "Stack Begin" => in_stack = true,
            "Stack End" => in_stack = false,
            _ if in_stack => frames.extend(frame(line)),
            _ => text.push(line),
        }
    }
    let text = text.join("\n").trim_end().to_string();
    let location = leading_location(&text).or_else(|| frames.first().map(|f| f.location.clone()));
    GameMessage {
        context,
        text,
        location,
        frames,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_splits_into_text_location_and_frames() {
        let message = "ServerScriptService.Later:4: attempt to index nil with 'field'\n    \
                       Stack Begin\n    Script 'ServerScriptService.Later', Line 4 - function go\n    \
                       Script 'ServerScriptService.Later', Line 9\n    Stack End";
        let parsed = parse(Context::Server, message);
        assert_eq!(
            parsed.text,
            "ServerScriptService.Later:4: attempt to index nil with 'field'"
        );
        let location = parsed.location.expect("location");
        assert_eq!(location.path, "ServerScriptService.Later");
        assert_eq!(location.line, 4);
        assert_eq!(location.short(), "Later:4");
        assert_eq!(parsed.frames.len(), 2);
        assert_eq!(parsed.frames[0].detail, "- function go");
        assert_eq!(parsed.frames[1].location.line, 9);
        assert_eq!(parsed.frames[1].detail, "");
    }

    #[test]
    fn a_print_has_no_location() {
        let parsed = parse(Context::Client, "hello: world");
        assert_eq!(parsed.text, "hello: world");
        assert_eq!(parsed.location, None);
        assert!(parsed.frames.is_empty());
    }

    #[test]
    fn an_error_without_a_prefix_takes_its_first_frame() {
        let message = "boom\nStack Begin\nScript 'Players.T.PlayerScripts.Hud', Line 3\nStack End";
        let parsed = parse(Context::Client, message);
        assert_eq!(parsed.text, "boom");
        let location = parsed.location.expect("location");
        assert_eq!(location.path, "Players.T.PlayerScripts.Hud");
        assert_eq!(location.line, 3);
    }
}
