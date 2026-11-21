//! Text model for the Rustaco editor.
//!
//! No UI and no I/O: a rope buffer, multi-cursor selections, change sets,
//! undo history and the editing commands. Frontends (web and native) and
//! services (syntax highlighting, LSP) consume the [`EditEvent`]s it emits.

mod change;
mod document;
mod history;
mod position;
mod selection;

pub use change::{Assoc, ChangeSet, Edit, EditEvent};
pub use document::{Document, LineEnding, Options, word_at};
pub use history::{EditKind, History, Transaction};
pub use position::{
    Point, Position, Utf16Position, char_to_point, char_to_position, char_to_utf16, line_end, line_len,
    position_to_char, utf16_to_char,
};
pub use ropey::Rope;
pub use selection::{Selection, Selections};
