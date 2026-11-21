//! Undo/redo history with grouping of consecutive typing.

use crate::change::ChangeSet;
use crate::editor::EditorId;
use crate::selection::Selections;

/// One committed change with what is needed to revert it.
#[derive(Clone, Debug)]
pub struct Transaction {
    pub changes: ChangeSet,
    pub inverse: ChangeSet,
    pub before: Selections,
    pub after: Selections,
    /// The editor that made the change; its selections are restored on undo.
    pub origin: EditorId,
}

/// Decides whether a transaction may join the previous undo step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// Single printable characters. Consecutive typing undoes as one step.
    Typing,
    /// Backspace and Delete. Consecutive deletions undo as one step.
    Deleting,
    /// Anything else: always its own undo step.
    Other,
}

/// One undo step: one or more transactions applied in order.
#[derive(Clone, Debug)]
pub struct UndoStep {
    pub transactions: Vec<Transaction>,
    kind: EditKind,
}

#[derive(Clone, Debug)]
pub struct History {
    undo: Vec<UndoStep>,
    redo: Vec<UndoStep>,
    group_open: bool,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::with_limit(1000)
    }
}

impl History {
    /// `limit` is the number of undo steps kept; older ones are dropped.
    pub fn with_limit(limit: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), group_open: false, limit: limit.max(1) }
    }

    pub fn push(&mut self, tx: Transaction, kind: EditKind) {
        self.redo.clear();
        match self.undo.last_mut() {
            Some(step)
                if self.group_open
                    && kind != EditKind::Other
                    && step.kind == kind
                    && step.transactions.last().is_some_and(|t| t.origin == tx.origin) =>
            {
                step.transactions.push(tx);
            }
            _ => {
                self.undo.push(UndoStep { transactions: vec![tx], kind });
                if self.undo.len() > self.limit {
                    self.undo.remove(0);
                }
            }
        }
        self.group_open = true;
    }

    /// Ends the current group, so the next edit starts a new undo step.
    /// Called on cursor movement, selection changes and explicit checkpoints.
    pub fn break_group(&mut self) {
        self.group_open = false;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(crate) fn pop_undo(&mut self) -> Option<UndoStep> {
        self.group_open = false;
        self.undo.pop()
    }

    pub(crate) fn pop_redo(&mut self) -> Option<UndoStep> {
        self.group_open = false;
        self.redo.pop()
    }

    pub(crate) fn push_redo(&mut self, step: UndoStep) {
        self.redo.push(step);
    }

    pub(crate) fn push_undo(&mut self, step: UndoStep) {
        self.undo.push(step);
    }
}
