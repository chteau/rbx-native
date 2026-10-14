//! The Output panel's "level of detail" toggles beyond Show Timestamp — the
//! rest of the list `studio/output.md` gives under its overflow menu.

/// Each field is one checkbox in the Output panel's overflow menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputDisplay {
    /// Show Context: a Play message's Server/Client tag.
    pub(crate) context: bool,
    /// Show Source: a Play error's `Script:line`.
    pub(crate) source: bool,
    /// Show Tables Expanded by Default: a printed table opens one level.
    pub(crate) tables_expanded: bool,
    /// Show Memory Address for Expandable Tables.
    pub(crate) table_addresses: bool,
    /// Log Mode: plain text, no expandable trees.
    pub(crate) log_mode: bool,
}

impl Default for OutputDisplay {
    /// Context and source on, the way the Play rows already read before
    /// either could be turned off; the table options off, as Studio ships.
    fn default() -> Self {
        OutputDisplay {
            context: true,
            source: true,
            tables_expanded: false,
            table_addresses: false,
            log_mode: false,
        }
    }
}

impl OutputDisplay {
    /// The toggles saved flat beside `output_timestamps`; a key the file
    /// lacks keeps its default.
    pub(super) fn read(value: &serde_json::Value) -> Self {
        let defaults = OutputDisplay::default();
        let flag =
            |key: &str, default: bool| value.get(key).and_then(|v| v.as_bool()).unwrap_or(default);
        OutputDisplay {
            context: flag("output_show_context", defaults.context),
            source: flag("output_show_source", defaults.source),
            tables_expanded: flag("output_tables_expanded", defaults.tables_expanded),
            table_addresses: flag("output_table_addresses", defaults.table_addresses),
            log_mode: flag("output_log_mode", defaults.log_mode),
        }
    }
}
