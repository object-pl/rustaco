// The diff editor: an original and a modified model side by side (or one editor with the
// deleted lines shown inline), changed lines and chars marked, both sides editable. The diff
// runs in the worker and again after edits; filler blocks keep the sides' lines aligned and
// the panes scroll together.

import { RustacoEditor, createModel, defineDecorationType } from './rustaco.js';
import { styleColors } from './pkg/rustaco_web.js';

const RECOMPUTE_MS = 200;

/** Decodes the flat diff from the editor module into change objects. */
function decode(flat) {
  if (!flat) return { complete: false, changes: [] };
  const changes = [];
  let i = 2;
  for (let n = 0; n < flat[1]; n++) {
    const c = {
      original: { start: flat[i], end: flat[i + 1] },
      modified: { start: flat[i + 2], end: flat[i + 3] },
      moved: flat[i + 4] ? flat[i + 4] - 1 : null,
      inner: [],
    };
    const count = flat[i + 5];
    i += 6;
    for (let k = 0; k < count; k++, i += 4) {
      c.inner.push({ original: { start: flat[i], end: flat[i + 1] }, modified: { start: flat[i + 2], end: flat[i + 3] } });
    }
    changes.push(c);
  }
  return { complete: flat[0] === 1, changes };
}

let types = null;
function diffTypes() {
  types ??= Promise.all([
    defineDecorationType({ background: '#ff000026', wholeLine: true, overviewRuler: '#ff0000aa', gutter: '#ff4444' }),
    defineDecorationType({ background: '#9bb95526', wholeLine: true, overviewRuler: '#9bb955aa', gutter: '#9bb955' }),
    defineDecorationType({ background: '#ff000055' }),
    defineDecorationType({ background: '#9ccc2c55' }),
  ]).then(([removedLine, insertedLine, removedText, insertedText]) => ({ removedLine, insertedLine, removedText, insertedText }));
  return types;
}

/**
 * Creates a diff editor in `container`. `original` and `modified` are texts or models;
 * `inline` shows one editor with the deleted lines between the lines. Other options go to
 * both editors.
 */
export async function createDiffEditor(container, { original = '', modified = '', language = '', inline = false, ignoreWhitespace = false, ...options } = {}) {
  const left = typeof original === 'string' ? await createModel(original, language) : original;
  const right = typeof modified === 'string' ? await createModel(modified, language) : modified;
  const editor = new RustacoDiffEditor(container, left, right, { inline, ignoreWhitespace, options });
  await editor.computeDiff();
  return editor;
}

export class RustacoDiffEditor {
  constructor(container, original, modified, { inline, ignoreWhitespace, options }) {
    this.container = container;
    this.originalModel = original;
    this.modifiedModel = modified;
    this.inline = inline;
    this.ignoreWhitespace = ignoreWhitespace;
    this.changes = [];
    this.zones = [];
    if (getComputedStyle(container).position === 'static') container.style.position = 'relative';
    if (inline) {
      this.modifiedPane = this.#pane('0', '0');
      this.modifiedEditor = new RustacoEditor(this.modifiedPane, { ...options, model: modified });
    } else {
      this.originalPane = this.#pane('0', '50%');
      this.modifiedPane = this.#pane('50%', '0');
      this.originalPane.style.borderRight = '1px solid #80808060';
      this.originalEditor = new RustacoEditor(this.originalPane, { ...options, model: original });
      this.modifiedEditor = new RustacoEditor(this.modifiedPane, { ...options, model: modified });
      // The sides scroll together; fillers make the same scroll position show the same lines.
      let syncing = false;
      const follow = (from, to) => from.onDidScrollChange((top) => {
        if (syncing) return;
        syncing = true;
        to.setScrollTop(top);
        syncing = false;
      });
      follow(this.originalEditor, this.modifiedEditor);
      follow(this.modifiedEditor, this.originalEditor);
    }
    this.originalDecorations = original.createDecorationsCollection();
    this.modifiedDecorations = modified.createDecorationsCollection();
    let timer = 0;
    const later = () => { clearTimeout(timer); timer = setTimeout(() => this.computeDiff(), RECOMPUTE_MS); };
    this.offs = [original.onDidChangeContent(later), modified.onDidChangeContent(later)];
  }

  #pane(left, right) {
    const pane = document.createElement('div');
    pane.style.cssText = `position:absolute;top:0;bottom:0;left:${left};right:${right}`;
    this.container.append(pane);
    return pane;
  }

  get editors() { return [this.originalEditor, this.modifiedEditor].filter(Boolean); }

  /** Diffs the models (in the worker where there is one) and redraws the marks. */
  async computeDiff() {
    const a = this.originalModel;
    const b = this.modifiedModel;
    const versions = [a.core.version(), b.core.version()];
    const la = a.workerLink();
    const lb = b.workerLink();
    let flat;
    if (la && lb) {
      const r = await la.diff(lb, { ignoreWhitespace: this.ignoreWhitespace, budget: 1000 });
      // An edit landed while the worker diffed: the next run catches up.
      if (r.version !== a.core.version() || r.otherVersion !== b.core.version()) return this.changes;
      flat = r.flat;
    } else {
      flat = a.core.diffWith(b.core, this.ignoreWhitespace, 1000);
    }
    if (versions[0] !== a.core.version() || versions[1] !== b.core.version()) return this.changes;
    const { changes, complete } = decode(flat);
    this.changes = changes;
    this.complete = complete;
    await this.#render();
    this.onDidUpdateDiff?.(changes);
    return changes;
  }

  async #render() {
    const t = await diffTypes();
    const lineRange = (model, lines) => ({
      start: model.offsetAt({ line: lines.start, column: 0 }),
      end: model.offsetAt({ line: Math.max(lines.start, lines.end - 1), column: 1 << 30 }),
    });
    const removed = [];
    const inserted = [];
    for (const c of this.changes) {
      if (c.original.end > c.original.start) removed.push({ ...lineRange(this.originalModel, c.original), type: t.removedLine });
      if (c.modified.end > c.modified.start) inserted.push({ ...lineRange(this.modifiedModel, c.modified), type: t.insertedLine });
      for (const i of c.inner) {
        if (i.original.end > i.original.start) removed.push({ ...i.original, type: t.removedText });
        if (i.modified.end > i.modified.start) inserted.push({ ...i.modified, type: t.insertedText });
      }
    }
    this.originalDecorations.set(this.inline ? [] : removed);
    this.modifiedDecorations.set(inserted);
    for (const [editor, id] of this.zones) editor.removeViewZone(id);
    this.zones = [];
    const lh = this.modifiedEditor.core.line_height();
    if (this.inline) {
      // Deleted lines as blocks above where they were, coloured like the original.
      for (const c of this.changes) {
        const n = c.original.end - c.original.start;
        if (!n) continue;
        const node = await this.#deletedLines(c.original);
        this.zones.push([this.modifiedEditor, this.modifiedEditor.addViewZone({ afterLine: c.modified.start - 1, height: n * lh, domNode: node })]);
      }
    } else {
      // Fillers: the shorter side of each change gets the missing height.
      for (const c of this.changes) {
        const diff = (c.original.end - c.original.start) - (c.modified.end - c.modified.start);
        if (diff > 0) {
          this.zones.push([this.modifiedEditor, this.modifiedEditor.addViewZone({ afterLine: c.modified.end - 1, height: diff * lh, domNode: filler() })]);
        } else if (diff < 0) {
          this.zones.push([this.originalEditor, this.originalEditor.addViewZone({ afterLine: c.original.end - 1, height: -diff * lh, domNode: filler() })]);
        }
      }
    }
    for (const e of this.editors) e.schedule();
  }

  /** DOM for deleted `lines` of the original, with its syntax colours where it has them. */
  async #deletedLines(lines) {
    const model = this.originalModel;
    const colors = styleColors();
    const quads = await model.highlightLines?.(lines.start, lines.end) ?? [];
    const node = document.createElement('div');
    node.setAttribute('role', 'region');
    node.setAttribute('aria-label', 'Deleted lines');
    node.style.cssText = 'background:#ff000026;white-space:pre;overflow:hidden;font:inherit;pointer-events:none';
    const font = this.modifiedEditor.fontCss();
    const lh = this.modifiedEditor.core.line_height();
    for (let line = lines.start; line < lines.end; line++) {
      const row = document.createElement('div');
      row.style.cssText = `height:${lh}px;line-height:${lh}px;font:${font};color:${colors[0] ?? 'inherit'}`;
      const text = model.getLineContent(line);
      const spans = [];
      for (let q = 0; q + 3 < quads.length; q += 4) if (quads[q] === line) spans.push([quads[q + 1], quads[q + 2], quads[q + 3]]);
      const chars = [...text];
      let at = 0;
      for (const [s, e, style] of spans) {
        if (s > at) row.append(chars.slice(at, s).join(''));
        const span = document.createElement('span');
        span.textContent = chars.slice(s, e).join('');
        span.style.color = colors[style] ?? '';
        row.append(span);
        at = Math.max(at, e);
      }
      if (at < chars.length) row.append(chars.slice(at).join(''));
      node.append(row);
    }
    return node;
  }

  /** Moves the modified editor's caret to the next (or previous) change. */
  goToChange(forward = true) {
    const e = this.modifiedEditor;
    const head = e.core.selections()[1];
    const line = this.modifiedModel.positionAt(head).line;
    const starts = this.changes.map((c) => c.modified.start);
    const target = forward ? starts.find((s) => s > line) ?? starts[0] : [...starts].reverse().find((s) => s < line) ?? starts[starts.length - 1];
    if (target === undefined) return false;
    const offset = this.modifiedModel.offsetAt({ line: target, column: 0 });
    e.setSelections([{ anchor: offset, head: offset }]);
    e.focus();
    return true;
  }

  /** Puts the original text of change `index` back into the modified model. */
  revertChange(index) {
    const c = this.changes[index];
    if (!c) return;
    const a = this.originalModel;
    const b = this.modifiedModel;
    const span = (model, lines) => ({ start: model.offsetAt({ line: lines.start, column: 0 }), end: lines.end >= model.getLineCount() ? model.getValue().length : model.offsetAt({ line: lines.end, column: 0 }) });
    const from = span(a, c.original);
    let text = a.getTextRange(from.start, from.end);
    const to = span(b, c.modified);
    // At the end of a file without a final newline the line breaks sit on the other side.
    if (to.end === b.getValue().length && from.end !== a.getValue().length && text.endsWith('\n') && to.start === to.end && to.start > 0) {
      text = `\n${text.slice(0, -1)}`;
    }
    this.modifiedEditor.features.applyEdits([{ range: to, text }]);
  }

  /** Folds unchanged stretches longer than `2 * context` lines on both sides. */
  collapseUnchanged(context = 3) {
    const regions = (side, model) => {
      const out = [];
      let at = 0;
      for (const c of this.changes) {
        if (c[side].start - at > 2 * context) out.push({ start: at + context - (at === 0 ? context : 0), end: c[side].start - context - 1 });
        at = c[side].end;
      }
      const last = model.getLineCount() - 1;
      if (last - at > 2 * context) out.push({ start: at + context, end: last });
      return out.filter((r) => r.end > r.start);
    };
    const apply = (editor, side, model) => {
      if (!editor) return;
      const rs = regions(side, model);
      editor.core.setFoldingRanges(Uint32Array.from(rs.flatMap((r) => [r.start, r.end])), false);
      editor.runCommand('editor.foldAll');
    };
    apply(this.originalEditor, 'original', this.originalModel);
    apply(this.modifiedEditor, 'modified', this.modifiedModel);
  }

  dispose() {
    for (const off of this.offs) off();
    for (const e of this.editors) e.dispose();
    this.originalPane?.remove();
    this.modifiedPane?.remove();
  }
}

function filler() {
  const node = document.createElement('div');
  node.style.cssText = 'background:repeating-linear-gradient(-45deg,#80808018 0 4px,transparent 4px 8px);pointer-events:none';
  return node;
}
