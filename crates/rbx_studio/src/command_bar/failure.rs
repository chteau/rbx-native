//! A failed run the way Studio's Output prints it: the error, then its stack
//! as rows of their own between `Stack Begin` and `Stack End`,
//!
//! ```text
//! CommandBar:2: boom
//! Stack Begin
//! Script 'CommandBar', Line 2 - function f
//! Script 'CommandBar', Line 4
//! Stack End
//! ```
//!
//! and, for the run the bar itself started, the error once more: Studio's
//! bar reports its own run failing after the script's error is printed.

use super::Feedback;

/// Chunks the runtime builds for itself (the run wrapper, the scheduler's
/// helpers, `require`): their frames are plumbing, not the user's code, and
/// Studio has nothing in their place.
const INTERNAL: [&str; 6] = ["run", "runner", "yielding", "require", "errors", "utf8"];

/// The Output rows for one error as the runtime reports it (`mlua`'s text:
/// a `runtime error: ` kind in front, `stack traceback:` and one frame per
/// line after). `again` repeats the error last, as the bar's own failure.
pub(crate) fn rows(text: &str, again: bool) -> Vec<Feedback> {
    let (message, frames) = parse(text);
    let mut rows = vec![Feedback::Error(message.clone())];
    if !frames.is_empty() {
        rows.push(Feedback::Info("Stack Begin".to_owned()));
        rows.extend(frames.into_iter().map(Feedback::Info));
        rows.push(Feedback::Info("Stack End".to_owned()));
    }
    if again {
        rows.push(Feedback::Error(message));
    }
    rows
}

/// The error without its kind or traceback: what Studio prints, and what the
/// bar's own label shows.
pub(crate) fn message(text: &str) -> String {
    parse(text).0
}

fn parse(text: &str) -> (String, Vec<String>) {
    let (message, traceback) = text.split_once("\nstack traceback:").unwrap_or((text, ""));
    let message = ["runtime error: ", "syntax error: ", "callback error: "]
        .iter()
        .find_map(|kind| message.strip_prefix(kind))
        .unwrap_or(message);
    let frames = traceback.lines().filter_map(frame).collect();
    (message.trim().to_owned(), frames)
}

/// `CommandBar:2: in function 'f'` → `Script 'CommandBar', Line 2 - function f`;
/// `None` for a C function or one of the runtime's own chunks.
fn frame(line: &str) -> Option<String> {
    let (chunk, rest) = line.trim().split_once(':')?;
    // `mlua`'s own Lua chunks (`__mlua_index`, the userdata `__index`
    // shim) are plumbing too.
    if chunk == "[C]" || INTERNAL.contains(&chunk) || chunk.starts_with("__mlua") {
        return None;
    }
    let (number, what) = rest.split_once(':').unwrap_or((rest, ""));
    let number: u32 = number.trim().parse().ok()?;
    // `in function 'f'`, `in upvalue 'f'`, `in method 'm'`: Roblox names any
    // of them `function f`. A main chunk or an anonymous function has none.
    let name = what
        .split_once('\'')
        .and_then(|(_, rest)| rest.strip_suffix('\''));
    Some(match name {
        Some(name) => format!("Script '{chunk}', Line {number} - function {name}"),
        None => format!("Script '{chunk}', Line {number}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAISED: &str = "runtime error: CommandBar:2: boom\nstack traceback:\n\
        [C]: in function 'error'\n\
        CommandBar:2: in function 'f'\n\
        CommandBar:4: in function <CommandBar:1>\n\
        run:1: in function <run:1>";

    #[test]
    fn a_runtime_error_prints_as_studio_does() {
        let info = |text: &str| Feedback::Info(text.to_owned());
        assert_eq!(
            rows(RAISED, true),
            [
                Feedback::Error("CommandBar:2: boom".into()),
                info("Stack Begin"),
                info("Script 'CommandBar', Line 2 - function f"),
                info("Script 'CommandBar', Line 4"),
                info("Stack End"),
                Feedback::Error("CommandBar:2: boom".into()),
            ]
        );
    }

    #[test]
    fn a_syntax_error_has_no_stack() {
        let text =
            "syntax error: CommandBar:2: Expected identifier when parsing expression, got '\\'";
        assert_eq!(
            rows(text, false),
            [Feedback::Error(
                "CommandBar:2: Expected identifier when parsing expression, got '\\'".into()
            )]
        );
    }

    #[test]
    fn the_label_is_the_message_alone() {
        assert_eq!(message(RAISED), "CommandBar:2: boom");
    }

    #[test]
    fn a_module_frame_keeps_its_script_name() {
        assert_eq!(
            frame("ReplicatedStorage.Mod:3: in upvalue 'go'").as_deref(),
            Some("Script 'ReplicatedStorage.Mod', Line 3 - function go")
        );
        assert_eq!(frame("yielding:12: in function <yielding:10>"), None);
    }

    #[test]
    fn mlua_s_own_chunks_are_left_out() {
        let text = "runtime error: CommandBar:1: Nope is not a valid member of StarterPlayer\n\
            stack traceback:\n\
            [C]: in function 'error'\n\
            __mlua_index:31: in function <__mlua_index:28>\n\
            CommandBar:1: in main chunk";
        let rows = rows(text, false);
        assert!(
            rows.iter()
                .all(|row| !format!("{row:?}").contains("__mlua")),
            "{rows:?}"
        );
        assert!(rows
            .iter()
            .any(|row| format!("{row:?}").contains("Script 'CommandBar', Line 1")));
    }
}
