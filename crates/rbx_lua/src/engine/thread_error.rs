//! A thread's error the way Roblox's Output prints it.

/// `error`, which a thread died of, as Roblox prints one: the message,
/// with where it was raised in front when a binding raised it (Roblox puts
/// the calling line on those too), then its stack in Roblox's layout,
/// the engine's and mlua's own frames left out.
pub(super) fn roblox_error(error: &mlua::Error) -> String {
    let text = error.to_string();
    let trace = text.split_once("\nstack traceback:").map_or("", |(_, t)| t);
    let frames: Vec<(&str, &str, Option<&str>)> = trace
        .lines()
        .filter_map(|line| {
            let (chunk, rest) = line.trim().split_once(':')?;
            if chunk == "[C]" || super::scheduler::internal_chunk(chunk) {
                return None;
            }
            let (number, what) = rest.split_once(':').unwrap_or((rest, ""));
            let number = number.trim();
            number.parse::<u32>().ok()?;
            let name = what
                .split_once('\'')
                .and_then(|(_, rest)| rest.strip_suffix('\''));
            Some((chunk, number, name))
        })
        .collect();
    let mut message = crate::runtime::globals::error_text(error);
    if let Some((cut, _)) = message.split_once("\nstack traceback:") {
        message = cut.to_owned();
    }
    if let (mlua::Error::CallbackError { .. }, Some((chunk, line, _))) = (error, frames.first()) {
        message = format!("{chunk}:{line}: {message}");
    }
    if frames.is_empty() {
        return message;
    }
    let mut lines = vec![message, "    Stack Begin".to_owned()];
    for (chunk, line, name) in frames {
        lines.push(match name {
            Some(name) => format!("    Script '{chunk}', Line {line} - function {name}"),
            None => format!("    Script '{chunk}', Line {line}"),
        });
    }
    lines.push("    Stack End".to_owned());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::roblox_error;

    // mlua's `CallbackError` holds its cause in an `Arc` whatever the error.
    #[allow(clippy::arc_with_non_send_sync)]
    #[test]
    fn a_binding_error_reads_as_roblox_s() {
        let error = mlua::Error::CallbackError {
            traceback: "stack traceback:\n\t[C]: in ?\n\t__mlua_index:31: in function <__mlua_index:15>\n\tCommandBar:1: in function <CommandBar:1>".into(),
            cause: Arc::new(mlua::Error::runtime(
                "Nope is not a valid member of Workspace \"Workspace\"",
            )),
        };
        assert_eq!(
            roblox_error(&error),
            "CommandBar:1: Nope is not a valid member of Workspace \"Workspace\"\n    \
             Stack Begin\n    Script 'CommandBar', Line 1\n    Stack End"
        );
    }

    #[test]
    fn a_raised_error_keeps_its_own_location() {
        let error = mlua::Error::runtime(
            "Workspace.Script:2: boom\nstack traceback:\n\t[C]: in function 'error'\n\tWorkspace.Script:2: in function 'go'\n\trun:1: in function <run:1>",
        );
        assert_eq!(
            roblox_error(&error),
            "Workspace.Script:2: boom\n    Stack Begin\n    \
             Script 'Workspace.Script', Line 2 - function go\n    Stack End"
        );
    }
}
