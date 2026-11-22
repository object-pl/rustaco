//! Text model for the Rustaco editor.
//!
//! No UI and no I/O: a rope buffer, multi-cursor selections, change sets,
//! undo history and the editing commands. A [`TextModel`] can be shared by
//! several editors, each with its own [`Cursors`]; [`Document`] bundles one
//! model with one editor for the common case. Frontends (web and native) and
//! services (syntax highlighting, LSP) consume the [`EditEvent`]s it emits.

mod change;
mod commands;
mod decorations;
mod diff;
mod document;
mod editor;
mod fuzzy;
mod history;
mod keymap;
mod model;
mod position;
mod search;
mod selection;
mod snippet;

pub use change::{Assoc, ChangeSet, Edit, EditEvent};
pub use commands::matching_bracket_text;
pub use decorations::{Decoration, DecorationId, Decorations, Stickiness};
pub use diff::{CharChange, Diff, DiffOptions, LineChange, diff};
pub use document::Document;
pub use editor::{Cursors, Editor, EditorId, word_at};
pub use fuzzy::{FuzzyMatch, collect_words, fuzzy_score, rank_completions};
pub use history::{EditKind, History, Transaction};
pub use keymap::{Binding, KeyPress, Keymap, Platform, Resolved, default_keymap, eval_when, jetbrains_keymap};
pub use model::{LanguageEditing, LineEnding, Options, Stale, TextModel};
pub use position::{
    Point, Position, Utf16Position, char_to_point, char_to_position, char_to_utf16, line_end, line_len,
    position_to_char, utf16_to_char,
};
pub use ropey::{Rope, RopeSlice};
pub use search::{SearchQuery, Searcher, next_match};
pub use selection::{Selection, Selections};
pub use snippet::{SNIPPET_OWNER, SNIPPET_STOP, Snippet, SnippetSession, TabStop};
