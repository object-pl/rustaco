// The live demo: one Rustaco editor (or diff editor) under the tabs, in the page's theme.

import { createEditor, setTheme } from './editor/rustaco.js';
import { createDiffEditor } from './editor/diff.js';

const RUST = `use std::collections::HashMap;

/// Counts the words of a text.
struct Counter {
    counts: HashMap<String, usize>,
}

impl Counter {
    fn new() -> Self {
        Counter { counts: HashMap::new() }
    }

    fn add(&mut self, word: &str) {
        *self.counts.entry(word.to_owned()).or_default() += 1;
    }

    fn total(&self) -> usize {
        self.counts.values().sum()
    }
}

fn main() {
    let mut counter = Counter::new();
    for word in "the quick brown fox jumps over the lazy dog".split_whitespace() {
        counter.add(word);
    }
    let total = counter.total();
    println!("{total} words");
}
`;

const MERMAID = `%% Order flow
graph TD
  order[Order placed] --> check{In stock?}
  check -->|yes| pack[(Warehouse)]
  check -->|no| wait([Back order])
  wait --> check
  pack --> ship((Shipped))
`;

const DIFF_BEFORE = `fn area(width: f64, height: f64) -> f64 {
    width * height
}

fn main() {
    println!("{}", area(2.0, 3.0));
}
`;

const DIFF_AFTER = `/// The area of a rectangle.
fn area(width: f64, height: f64) -> f64 {
    assert!(width >= 0.0 && height >= 0.0);
    width * height
}

fn main() {
    let a = area(2.0, 3.0);
    println!("area: {a}");
}
`;

const HINTS = {
  rust: 'Ctrl+Space completes, F12 goes to a definition, F2 renames a local, Ctrl+Shift+O lists the symbols.',
  mermaid: 'Ctrl+Space on an empty line offers the node ids and keywords; after "graph " it offers the directions.',
  diff: 'Both sides are editable; the diff runs again 200 ms after an edit. Alt+F5 steps to the next change.',
};

const host = document.getElementById('demo-editor');
const hint = document.getElementById('demo-hint');
const font = { fontFamily: '"IBM Plex Mono", Consolas, monospace', fontSize: 14 };
let current = null;

function pageTheme() {
  return document.documentElement.dataset.theme === 'light' ? 'light' : 'dark';
}

async function show(kind) {
  current?.dispose?.();
  current = null;
  host.replaceChildren();
  hint.textContent = HINTS[kind];
  if (kind === 'diff') {
    current = await createDiffEditor(host, { original: DIFF_BEFORE, modified: DIFF_AFTER, language: 'rust', ...font });
  } else {
    current = await createEditor(host, { value: kind === 'rust' ? RUST : MERMAID, language: kind, ...font });
  }
}

for (const tab of document.querySelectorAll('[data-demo]')) {
  tab.addEventListener('click', () => {
    for (const t of document.querySelectorAll('[data-demo]')) {
      const on = t === tab;
      t.classList.toggle('active', on);
      t.setAttribute('aria-selected', String(on));
    }
    show(tab.dataset.demo);
  });
}

await setTheme(pageTheme());
new MutationObserver(() => setTheme(pageTheme())).observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
show('rust');
