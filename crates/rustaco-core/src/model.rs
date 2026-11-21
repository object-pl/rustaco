//! The shared text model: buffer, version, undo history and the change log.
//!
//! Several editors can show one model, each with its own [`crate::Cursors`].
//! Consumers that follow the text (other editors' cursors, syntax trees, LSP
//! clients) read the change log by version instead of being notified, so the
//! model holds no references to them.

use std::collections::VecDeque;

use ropey::Rope;

use crate::change::{ChangeSet, Edit, EditEvent};
use crate::history::History;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
    Cr,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }

    /// The first line terminator in the text decides; text without one gets `Lf`.
    pub fn detect(rope: &Rope) -> Self {
        let mut chars = rope.chars();
        while let Some(c) = chars.next() {
            match c {
                '\n' => return LineEnding::Lf,
                '\r' => return if chars.next() == Some('\n') { LineEnding::CrLf } else { LineEnding::Cr },
                _ => {}
            }
        }
        LineEnding::Lf
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub tab_size: usize,
    pub insert_spaces: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { tab_size: 4, insert_spaces: true }
    }
}

/// Language-specific editing data, set by the frontend from its language catalog: comment
/// tokens for comment toggling, bracket and quote pairs for auto-closing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LanguageEditing {
    pub line_comment: Option<String>,
    pub block_comment: Option<(String, String)>,
    pub auto_closing: Vec<(String, String)>,
}

/// Change sets kept in the log. A consumer further behind than this must resync from the full text.
const LOG_CAPACITY: usize = 1024;

/// Returned when a consumer asks for changes older than the log keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stale;

#[derive(Clone, Debug)]
struct LogEntry {
    changes: ChangeSet,
    events: Vec<EditEvent>,
}

#[derive(Clone, Debug)]
pub struct TextModel {
    pub(crate) rope: Rope,
    pub(crate) history: History,
    pub(crate) line_ending: LineEnding,
    pub options: Options,
    pub editing: LanguageEditing,
    version: u64,
    log: VecDeque<LogEntry>,
    /// Version of the text before the oldest log entry.
    log_base: u64,
}

impl TextModel {
    pub fn new(text: &str) -> Self {
        let rope = Rope::from_str(text);
        Self {
            line_ending: LineEnding::detect(&rope),
            rope,
            history: History::default(),
            options: Options::default(),
            editing: LanguageEditing::default(),
            version: 0,
            log: VecDeque::new(),
            log_base: 0,
        }
    }

    pub fn rope(&self) -> &Rope {
        &self.rope
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }

    /// Increases by one with every applied change set, including undo and redo.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Ends the current undo group explicitly.
    pub fn checkpoint(&mut self) {
        self.history.break_group();
    }

    /// Replaces the whole text and clears the undo history (like Monaco's `setValue`).
    /// The replacement goes through the log, so cursors and other consumers follow it.
    pub fn set_text(&mut self, text: &str) {
        let all = ChangeSet::new(vec![Edit::new(0, self.rope.len_chars(), text)]);
        self.apply(&all);
        self.line_ending = LineEnding::detect(&self.rope);
        self.history = History::default();
    }

    /// Change sets applied after `version`, oldest first.
    pub fn changes_since(&self, version: u64) -> Result<impl Iterator<Item = &ChangeSet>, Stale> {
        Ok(self.log_since(version)?.map(|e| &e.changes))
    }

    /// Edit events applied after `version`, in application order.
    pub fn events_since(&self, version: u64) -> Result<impl Iterator<Item = &EditEvent>, Stale> {
        Ok(self.log_since(version)?.flat_map(|e| e.events.iter()))
    }

    fn log_since(&self, version: u64) -> Result<std::collections::vec_deque::Iter<'_, LogEntry>, Stale> {
        if version < self.log_base || version > self.version {
            return Err(Stale);
        }
        Ok(self.log.range((version - self.log_base) as usize..))
    }

    /// The only place the rope changes. Returns the inverse change set.
    ///
    /// Undo inverses can land inside a CRLF pair that a deletion created, so every change
    /// set is snapped here, not only new edits.
    pub(crate) fn apply(&mut self, changes: &ChangeSet) -> ChangeSet {
        if changes.needs_crlf_snap(&self.rope) {
            let snapped = changes.clone().snap_crlf(&self.rope);
            return self.apply(&snapped);
        }
        let events = changes.describe(&self.rope);
        let inverse = changes.apply(&mut self.rope);
        self.version += 1;
        self.log.push_back(LogEntry { changes: changes.clone(), events });
        if self.log.len() > LOG_CAPACITY {
            self.log.pop_front();
            self.log_base += 1;
        }
        inverse
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_serves_consumers_by_version() {
        let mut m = TextModel::new("abc");
        m.apply(&ChangeSet::new(vec![Edit::insert(0, "x")]));
        m.apply(&ChangeSet::new(vec![Edit::insert(4, "y")]));
        assert_eq!(m.text(), "xabcy");
        assert_eq!(m.changes_since(0).unwrap().count(), 2);
        assert_eq!(m.events_since(1).unwrap().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["y"]);
        assert_eq!(m.changes_since(2).unwrap().count(), 0);
        assert!(m.changes_since(3).is_err());
    }

    #[test]
    fn log_drops_old_entries() {
        let mut m = TextModel::new("");
        for _ in 0..LOG_CAPACITY + 5 {
            m.apply(&ChangeSet::new(vec![Edit::insert(0, "a")]));
        }
        assert!(m.changes_since(0).is_err());
        assert_eq!(m.changes_since(5).unwrap().count(), LOG_CAPACITY);
    }
}
