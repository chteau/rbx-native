//! The middleware lint (`lib/lint.js`): a Luau tokenizer just deep enough to
//! read a Service's shape, the per-file checks, and `loren doctor`'s pass.

mod analyze;
mod colon;
mod decl;
mod doctor;
mod dupes;
mod scan;
mod syntax;
mod tokenize;

pub use analyze::{
    analyze_luau, is_networked, ClientInfo, ClientMethod, LuauInfo, NameList, Named, ServiceKey,
};
pub use doctor::{project_layout, run_doctor, Layout};
pub use dupes::{duplicate_names, is_class_member, DupeIssue, KIND as DUPLICATE_NAME};
pub use scan::{
    apply_edits, is_script_file, levenshtein, module_name_of, scan_text, suggestion, Edit, Issue,
    Severity,
};
pub use tokenize::{structure, tokenize, Structure, Token, TokenKind};
