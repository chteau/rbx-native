//! What a plugin asks the user: a Color3 (`plugin:PromptColor3`), a
//! BrickColor (`plugin:PromptBrickColor`), whether it may reach an HTTP
//! host, or which item of a `PluginMenu`.

use gpui_kit::*;
use rbx_lua::plugin::{PluginId, PromptKind, UiId};

use crate::shell::Shell;

/// Prompts waiting for the user, oldest first; one shows at a time.
#[derive(Default)]
pub(crate) struct Prompts {
    pub(crate) queue: Vec<(UiId, PluginId, PromptKind)>,
}

impl Shell {
    /// Queues a prompt from `Request::Prompt`.
    pub(super) fn plugin_prompt(&mut self, id: UiId, plugin: PluginId, kind: PromptKind, cx: &mut Context<Self>) {
        todo!()
    }

    /// The front prompt as an overlay: a dialog with a colour picker (Color3)
    /// or the BrickColor palette (BrickColor) and OK/Cancel; an Allow/Deny
    /// dialog naming the plugin and host (HTTP); a menu at the cursor
    /// (Menu). Answering sends `Input::PromptAnswered` and shows the next.
    pub(super) fn plugin_prompt_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        todo!()
    }
}
