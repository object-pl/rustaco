//! Parsing and highlighting on a background thread, for native frontends.
//!
//! The browser runs the same work in a Web Worker (`web/rustaco-worker.js`); both follow one
//! protocol: the model's edit events go to the background as they happen, requests carry
//! the model version they are for, and replies computed for an older version are dropped.
//! The thread parses in slices and answers requests between them, from the last tree
//! shifted by the edits while a parse is still under way.

use std::ops::Range;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rustaco_core::{EditEvent, Rope, TextModel};

use crate::{Highlights, LanguageDef, Syntax};

/// Parse time per slice before the thread looks at its messages again.
const SLICE: Duration = Duration::from_millis(10);

enum Request {
    Reset { text: String, version: u64 },
    Edits { events: Vec<EditEvent>, version: u64 },
    Highlight { lines: Range<usize> },
    Stop,
}

/// Highlights computed in the background for `lines` of the model at `version`.
#[derive(Debug)]
pub struct HighlightReply {
    pub version: u64,
    pub lines: Range<usize>,
    pub highlights: Highlights,
    /// False while the tree still lags the text (colours of edited lines may be off).
    pub parsed: bool,
}

/// A language's parser on its own thread, kept in step with one model.
pub struct BackgroundSyntax {
    tx: Sender<Request>,
    rx: Receiver<HighlightReply>,
    /// Model version whose edits the thread has.
    sent: u64,
    thread: Option<JoinHandle<()>>,
}

impl BackgroundSyntax {
    /// Starts the thread with the model's current text.
    pub fn spawn(language: &'static LanguageDef, model: &TextModel) -> Result<Self, String> {
        // Fail here, not on the thread, when the grammar cannot load.
        Syntax::new(language)?;
        let (tx, requests) = channel();
        let (replies, rx) = channel();
        let thread = std::thread::Builder::new()
            .name(format!("rustaco-syntax-{}", language.id))
            .spawn(move || run(language, requests, replies))
            .map_err(|e| e.to_string())?;
        let me = Self { tx, rx, sent: model.version(), thread: Some(thread) };
        me.send(Request::Reset { text: model.text(), version: model.version() });
        Ok(me)
    }

    fn send(&self, request: Request) {
        // A send fails only when the thread has gone; replies then stop, which callers see.
        let _ = self.tx.send(request);
    }

    /// Sends the model's edits since the last call, or its whole text when the model's log
    /// no longer reaches back that far.
    pub fn sync(&mut self, model: &TextModel) {
        if model.version() == self.sent {
            return;
        }
        match model.events_since(self.sent) {
            Ok(events) => self.send(Request::Edits { events: events.cloned().collect(), version: model.version() }),
            Err(_) => self.send(Request::Reset { text: model.text(), version: model.version() }),
        }
        self.sent = model.version();
    }

    /// Asks for highlights of `lines` as the model is now.
    pub fn request_highlights(&mut self, model: &TextModel, lines: Range<usize>) {
        self.sync(model);
        self.send(Request::Highlight { lines });
    }

    /// The newest reply computed for the model's current version, if one has arrived.
    /// Replies for older versions are dropped.
    pub fn poll(&self, model: &TextModel) -> Option<HighlightReply> {
        let mut latest = None;
        while let Ok(reply) = self.rx.try_recv() {
            if reply.version == model.version() {
                latest = Some(reply);
            }
        }
        latest
    }

    /// Waits up to `timeout` for a reply for the model's current version.
    pub fn wait(&self, model: &TextModel, timeout: Duration) -> Option<HighlightReply> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            match self.rx.recv_timeout(left) {
                Ok(reply) if reply.version == model.version() => return Some(reply),
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    }
}

impl Drop for BackgroundSyntax {
    fn drop(&mut self) {
        self.send(Request::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(language: &'static LanguageDef, requests: Receiver<Request>, replies: Sender<HighlightReply>) {
    let Ok(mut syntax) = Syntax::new(language) else { return };
    let mut rope = Rope::new();
    let mut version = 0;
    let mut parsed = true;
    loop {
        // Block for work when the tree is current; otherwise only look, and parse on.
        let request = if parsed {
            match requests.recv() {
                Ok(r) => Some(r),
                Err(_) => return,
            }
        } else {
            match requests.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        };
        match request {
            Some(Request::Reset { text, version: v }) => {
                rope = Rope::from_str(&text);
                syntax.reset();
                version = v;
                parsed = false;
            }
            Some(Request::Edits { events, version: v }) => {
                for e in &events {
                    rope.remove(e.start..e.old_end);
                    rope.insert(e.start, &e.text);
                    syntax.apply_edit(e);
                }
                version = v;
                parsed = false;
            }
            Some(Request::Highlight { lines }) => {
                // A short parse first, so small edits come back with their final colours.
                let deadline = Instant::now() + Duration::from_millis(4);
                parsed = syntax.parse_with_budget(&rope, || Instant::now() < deadline);
                let end = lines.end.min(rope.len_lines());
                let start = lines.start.min(end);
                let highlights = syntax.highlight(&rope, start..end);
                if replies.send(HighlightReply { version, lines: start..end, highlights, parsed }).is_err() {
                    return;
                }
            }
            Some(Request::Stop) => return,
            None => {
                let deadline = Instant::now() + SLICE;
                parsed = syntax.parse_with_budget(&rope, || Instant::now() < deadline);
            }
        }
    }
}
