// The UI of language features other than completion: hover cards, parameter hints, go to
// definition, the references peek view, rename, formatting, code actions with a lightbulb,
// highlights of a symbol's uses, quick outline and code lenses. Each asks the providers
// registered for the model's language (see languages.js).

import { CancellationTokenSource, providersFor } from './languages.js';
import { parseMarkdown } from './pkg/rustaco_web.js';

const HOVER_DELAY_MS = 300;
const CARET_DELAY_MS = 250;

/**
 * First non-empty answer of the providers of `kind` to `method`, called with `args` and
 * `token` last; null when none answers or the token was cancelled meanwhile.
 */
async function ask(kind, model, token, method, ...args) {
  for (const p of providersFor(kind, model.getLanguage())) {
    if (typeof p[method] !== 'function') continue;
    try {
      const r = await p[method](model, ...args, token);
      if (token.isCancellationRequested) return null;
      if (r && (!Array.isArray(r) || r.length)) return r;
    } catch {
      // A failing provider gives no answer; the next one may.
    }
  }
  return null;
}

/** Every provider of `kind` that has `method`, asked at once; lists flattened, failures empty. */
async function askAll(kind, model, token, method, ...args) {
  const lists = await Promise.all(providersFor(kind, model.getLanguage()).map(async (p) => {
    try { return (await p[method](model, ...args, token)) ?? []; } catch { return []; }
  }));
  return token.isCancellationRequested ? null : lists.flat();
}

/** A range from a provider result: `{ start, end }` itself or a `{ range }` wrapper. */
const rangeOf = (r) => (r && 'range' in r ? r.range : r);

/**
 * The markdown hover and completion documentation use, as DOM: ``` fences as code blocks,
 * and in text `code`, **bold** and *italic*. Other markdown shows as written. Rust parses
 * (`parseMarkdown`); this builds the nodes.
 */
function renderMarkdown(parent, text, monoFont) {
  for (const [kind, content] of JSON.parse(parseMarkdown(String(text)))) {
    if (kind === 'pre') {
      const pre = document.createElement('pre');
      pre.textContent = content;
      pre.style.cssText = `margin:4px 0;font:12px ${monoFont}`;
      parent.append(pre);
      continue;
    }
    const div = document.createElement('div');
    div.style.cssText = 'white-space:pre-wrap;margin:2px 0';
    for (const [piece, value] of content) {
      if (piece === 'text') { div.append(value); continue; }
      const el = document.createElement(piece);
      if (piece === 'code') el.style.font = `12px ${monoFont}`;
      el.textContent = value;
      div.append(el);
    }
    parent.append(div);
  }
}

export class Features {
  constructor(editor, colors) {
    this.editor = editor;
    this.colors = colors;
    this.hoverTimer = 0;
    this.caretTimer = 0;
    this.request = 0;
    this.sources = new Map();
  }

  get model() { return this.editor.getModel(); }

  /** A fresh cancellation token for a request of `kind`, cancelling the previous one. */
  token(kind) {
    this.sources.get(kind)?.cancel();
    const source = new CancellationTokenSource();
    this.sources.set(kind, source);
    return source.token;
  }

  /** Cancels every request in flight (the model or the language changed). */
  cancelAll() {
    for (const source of this.sources.values()) source.cancel();
    this.sources.clear();
  }

  has(kind) { return providersFor(kind, this.model.getLanguage()).length > 0; }

  /** A floating box styled from the theme, for hover cards and hints. */
  #box(role, label) {
    const [background, foreground, border] = this.colors();
    const box = document.createElement('div');
    box.setAttribute('role', role);
    box.setAttribute('aria-label', label);
    box.style.cssText = `background:${background};color:${foreground};border:1px solid ${border};padding:4px 8px;` +
      'font:13px system-ui,sans-serif;max-width:520px;max-height:300px;overflow:auto';
    return box;
  }

  // ---- commands ----

  /** Runs a feature command; false when `command` is not one of them. */
  run(command) {
    const actions = {
      'editor.action.showHover': () => this.showHover(this.editor.core.selections()[1]),
      'editor.action.triggerParameterHints': () => this.signatureHelp(true),
      'editor.action.revealDefinition': () => this.goToDefinition(this.editor.core.selections()[1]),
      'editor.action.referenceSearch.trigger': () => this.peekReferences(),
      'editor.action.rename': () => this.rename(),
      'editor.action.formatDocument': () => this.format(),
      'editor.action.quickFix': () => this.quickFix(),
      'editor.action.quickOutline': () => this.quickOutline(),
    };
    const action = actions[command];
    if (!action) return false;
    action();
    return true;
  }

  // ---- hover ----

  /** Pointer moved over the canvas without a button held. */
  mouseMove(x, y) {
    clearTimeout(this.hoverTimer);
    const offset = this.editor.core.textOffsetAt(x, y);
    if (offset < 0) { this.#scheduleHideHover(); return; }
    if (this.hover && offset >= this.hover.range.start && offset < this.hover.range.end) return;
    this.hoverTimer = setTimeout(() => this.showHover(offset), HOVER_DELAY_MS);
  }

  mouseLeave() {
    clearTimeout(this.hoverTimer);
    this.#scheduleHideHover();
  }

  #scheduleHideHover() {
    if (!this.hover) return;
    clearTimeout(this.hideTimer);
    this.hideTimer = setTimeout(() => { if (!this.hover?.pointerInside) this.hideHover(); }, 200);
  }

  async showHover(offset) {
    const asked = ++this.request;
    const result = await ask('hover', this.model, this.token('hover'), 'provideHover', offset);
    if (asked !== this.request) return;
    this.hideHover();
    if (!result) return;
    const contents = Array.isArray(result.contents) ? result.contents : [result.contents];
    const box = this.#box('tooltip', 'Hover');
    contents.forEach((c, i) => {
      if (i > 0) box.append(document.createElement('hr'));
      renderMarkdown(box, c?.value ?? c, 'monospace');
    });
    const range = rangeOf(result) ?? { start: offset, end: offset + 1 };
    const widget = { domNode: box, position: range.start, preference: 'above' };
    this.hover = { widget, range, pointerInside: false };
    box.addEventListener('mouseenter', () => { if (this.hover) this.hover.pointerInside = true; });
    box.addEventListener('mouseleave', () => { if (this.hover) this.hover.pointerInside = false; this.#scheduleHideHover(); });
    this.editor.addContentWidget(widget);
  }

  hideHover() {
    if (!this.hover) return;
    this.editor.removeContentWidget(this.hover.widget);
    this.hover = null;
  }

  // ---- parameter hints ----

  /** After text was typed: open, update or close the parameter hints. */
  typed(text) {
    const last = [...text].pop() ?? '';
    const triggers = providersFor('signatureHelp', this.model.getLanguage())
      .flatMap((p) => [...(p.signatureHelpTriggerCharacters ?? []), ...(p.signatureHelpRetriggerCharacters ?? [])]);
    if (triggers.includes(last) || this.signature) this.signatureHelp(false);
  }

  async signatureHelp(explicit) {
    const offset = this.editor.core.selections()[1];
    const asked = ++this.request;
    const help = await ask('signatureHelp', this.model, this.token('signatureHelp'), 'provideSignatureHelp', offset, { explicit });
    if (asked !== this.request) return;
    this.closeSignature();
    const signature = help?.signatures?.[help.activeSignature ?? 0];
    if (!signature) return;
    const box = this.#box('tooltip', 'Parameter hints');
    const head = document.createElement('div');
    head.style.fontFamily = 'monospace';
    const active = signature.parameters?.[help.activeParameter ?? 0];
    const label = signature.label;
    const at = active ? label.indexOf(active.label) : -1;
    if (at >= 0) {
      const b = document.createElement('b');
      b.textContent = active.label;
      b.style.textDecoration = 'underline';
      head.append(label.slice(0, at), b, label.slice(at + active.label.length));
    } else {
      head.textContent = label;
    }
    box.append(head);
    const doc = active?.documentation ?? signature.documentation;
    if (doc) renderMarkdown(box, doc?.value ?? doc, 'monospace');
    if (help.signatures.length > 1) {
      const count = document.createElement('div');
      count.textContent = `${(help.activeSignature ?? 0) + 1} of ${help.signatures.length}`;
      count.style.opacity = '.6';
      box.prepend(count);
    }
    const widget = { domNode: box, position: offset, preference: 'above' };
    this.signature = widget;
    this.editor.addContentWidget(widget);
  }

  closeSignature() {
    if (!this.signature) return;
    this.editor.removeContentWidget(this.signature);
    this.signature = null;
  }

  // ---- definition, references ----

  async goToDefinition(offset) {
    const result = await ask('definition', this.model, this.token('definition'), 'provideDefinition', offset);
    const first = Array.isArray(result) ? result[0] : result;
    if (first && 'uri' in first && first.model !== this.model) {
      // Another document: the host opens it.
      this.editor.onOpenLocation?.(first);
      return true;
    }
    const range = rangeOf(first);
    if (!range) return false;
    this.editor.setSelections([{ anchor: range.start, head: range.end }]);
    this.editor.focus();
    return true;
  }

  /** Shows the references under the caret's line in an inline list (a view zone). */
  async peekReferences() {
    const e = this.editor;
    const offset = e.core.selections()[1];
    const result = await ask('references', this.model, this.token('references'), 'provideReferences', offset, { includeDeclaration: true });
    this.closePeek();
    if (!result) return;
    const ranges = result.map(rangeOf).filter(Boolean).sort((a, b) => a.start - b.start);
    const [background, foreground, border, , match] = this.colors();
    const node = document.createElement('div');
    node.setAttribute('role', 'listbox');
    node.setAttribute('aria-label', 'References');
    node.style.cssText = `background:${background};color:${foreground};border-top:1px solid ${match};` +
      `border-bottom:1px solid ${match};overflow:auto;font:13px system-ui,sans-serif;box-sizing:border-box`;
    const title = document.createElement('div');
    title.textContent = `${ranges.length} reference${ranges.length === 1 ? '' : 's'}`;
    title.style.cssText = `padding:2px 8px;border-bottom:1px solid ${border};opacity:.8`;
    node.append(title);
    // Line, the text around each match and the match itself, cut in chars by Rust.
    const rows = JSON.parse(this.model.core.referenceRows(Uint32Array.from(ranges.flatMap((r) => [r.start, r.end]))));
    for (const r of rows) {
      const row = document.createElement('div');
      row.setAttribute('role', 'option');
      row.style.cssText = 'padding:1px 8px;cursor:pointer;white-space:pre;font-family:monospace';
      const lineLabel = document.createElement('span');
      lineLabel.textContent = `${r.line + 1}: `;
      lineLabel.style.opacity = '.6';
      const b = document.createElement('b');
      b.textContent = r.match;
      b.style.color = match;
      row.append(lineLabel, r.pre, b, r.post);
      row.addEventListener('mousedown', (ev) => {
        ev.preventDefault();
        e.setSelections([{ anchor: r.start, head: r.end }]);
        this.closePeek();
        e.focus();
      });
      node.append(row);
    }
    const lh = e.core.line_height();
    const line = this.model.positionAt(offset).line;
    const id = e.addViewZone({ afterLine: line, height: Math.min(ranges.length, 8) * 20 + 26, domNode: node });
    this.peek = { id };
    void lh;
  }

  closePeek() {
    if (!this.peek) return false;
    this.editor.removeViewZone(this.peek.id);
    this.peek = null;
    return true;
  }

  // ---- rename, formatting, edits ----

  /** Applies provider edits `[{ range, text }]` as one undo step. */
  applyEdits(edits) {
    if (!edits?.length) return;
    // Rust sorts the edits (ChangeSet).
    const list = edits.map((x) => ({ range: rangeOf(x), text: x.text ?? x.newText ?? '' }));
    this.editor.core.applyEdits(Uint32Array.from(list.flatMap((x) => [x.range.start, x.range.end])), list.map((x) => x.text));
    this.editor.afterEdit();
  }

  async rename() {
    const e = this.editor;
    const offset = e.core.selections()[1];
    let target = null;
    const token = this.token('renameLocation');
    for (const p of providersFor('rename', this.model.getLanguage())) {
      if (p.resolveRenameLocation) {
        const r = await p.resolveRenameLocation(this.model, offset, token);
        if (token.isCancellationRequested) return;
        if (r?.rejectReason) { this.#flash(r.rejectReason, offset); return; }
        if (r) { target = { range: rangeOf(r), text: r.text }; break; }
      }
    }
    if (!target) {
      const [start, end] = e.core.wordAtCaret();
      if (start === end) return;
      target = { range: { start, end }, text: this.model.getTextRange(start, end) };
    }
    const [background, foreground, border] = this.colors();
    const input = document.createElement('input');
    input.value = target.text;
    input.setAttribute('aria-label', 'Rename');
    input.spellcheck = false;
    input.style.cssText = `background:${background};color:${foreground};border:1px solid ${border};font:13px monospace;padding:2px 4px;min-width:180px`;
    const widget = { domNode: input, position: target.range.start, preference: 'below' };
    e.addContentWidget(widget);
    input.focus();
    input.select();
    let finished = false;
    const finish = async (apply) => {
      // Removing the focused box blurs it, which calls this again.
      if (finished) return;
      finished = true;
      e.removeContentWidget(widget);
      e.focus();
      if (!apply || input.value === target.text || !input.value) return;
      const edits = await ask('rename', this.model, this.token('rename'), 'provideRenameEdits', offset, input.value);
      this.applyEdits(edits?.edits ?? edits);
    };
    input.addEventListener('keydown', (ev) => {
      if (ev.key === 'Enter') { ev.preventDefault(); finish(true); }
      if (ev.key === 'Escape') { ev.preventDefault(); finish(false); }
    });
    input.addEventListener('blur', () => finish(false));
  }

  async format() {
    const o = this.editor.getModel();
    const edits = await ask('formatting', o, this.token('formatting'), 'provideDocumentFormattingEdits', { tabSize: 4, insertSpaces: true });
    this.applyEdits(edits);
  }

  /** A short message next to `offset`, gone after two seconds. */
  #flash(message, offset) {
    const box = this.#box('status', 'Message');
    box.textContent = message;
    const widget = { domNode: box, position: offset, preference: 'below' };
    this.editor.addContentWidget(widget);
    setTimeout(() => this.editor.removeContentWidget(widget), 2000);
  }

  // ---- caret: code actions, highlights ----

  /** The caret or selection moved, or the text changed: refresh what depends on it. */
  caretMoved() {
    clearTimeout(this.caretTimer);
    this.caretTimer = setTimeout(() => {
      this.#updateLightbulb();
      this.#updateHighlights();
    }, CARET_DELAY_MS);
  }

  async #codeActions() {
    const [start, end] = this.editor.core.primaryRange();
    return (await askAll('codeAction', this.model, this.token('codeAction'), 'provideCodeActions', { start, end })) ?? [];
  }

  async #updateLightbulb() {
    const e = this.editor;
    if (!this.has('codeAction')) { this.#removeLightbulb(); return; }
    const actions = await this.#codeActions();
    this.#removeLightbulb();
    if (!actions.length) return;
    const bulb = document.createElement('div');
    bulb.textContent = '\u{1F4A1}';
    bulb.title = 'Show code actions (Ctrl+.)';
    bulb.setAttribute('role', 'button');
    bulb.setAttribute('aria-label', 'Code actions');
    bulb.style.cssText = 'cursor:pointer;font-size:12px;line-height:1';
    bulb.addEventListener('mousedown', (ev) => { ev.preventDefault(); this.quickFix(); });
    const head = e.core.selections()[1];
    const lineStart = this.model.offsetAt({ line: this.model.positionAt(head).line, column: 0 });
    // Placed at the line's start, pulled left over the fold marker column.
    const widget = { domNode: bulb, position: lineStart, preference: 'above', offsetX: -18 };
    bulb.style.transform = `translate(-18px, ${e.core.line_height()}px)`;
    this.lightbulb = widget;
    e.addContentWidget(widget);
  }

  #removeLightbulb() {
    if (!this.lightbulb) return;
    this.editor.removeContentWidget(this.lightbulb);
    this.lightbulb = null;
  }

  async quickFix() {
    const actions = await this.#codeActions();
    const e = this.editor;
    if (!actions.length) { this.#flash('No code actions available', e.core.selections()[1]); return; }
    this.quickPick(actions.map((a) => ({ label: a.title, action: a })), (item) => {
      const a = item.action;
      if (a.edits) this.applyEdits(a.edits);
      a.command?.(e);
    }, 'Code actions');
  }

  async #updateHighlights() {
    const e = this.editor;
    const has = this.has('documentHighlight');
    e.setOccurrencesFromProvider(has);
    if (!has) { this.highlights?.clear(); return; }
    const result = await ask('documentHighlight', this.model, this.token('documentHighlight'), 'provideDocumentHighlights', e.core.selections()[1]);
    this.highlights ??= this.model.createDecorationsCollection();
    const type = await e.highlightDecorationType();
    this.highlights.set((result ?? []).map(rangeOf).filter(Boolean).map((r) => ({ start: r.start, end: r.end, type })));
  }

  // ---- outline, quick pick ----

  async quickOutline() {
    const symbols = await this.editor.getDocumentSymbols();
    const flat = [];
    const walk = (list, depth) => {
      for (const s of list) {
        flat.push({ label: `${'  '.repeat(depth)}${s.name}`, detail: s.detail ?? s.kindName ?? '', symbol: s });
        if (s.children) walk(s.children, depth + 1);
      }
    };
    walk(symbols, 0);
    this.quickPick(flat, (item) => {
      const r = item.symbol.selectionRange ?? item.symbol.range;
      this.editor.setSelections([{ anchor: r.start, head: r.start }]);
    }, 'Go to symbol');
  }

  /**
   * A filterable list at the top of the editor: `items` are `{ label, detail? }`; Enter or a
   * click calls `pick(item)`.
   */
  quickPick(items, pick, label) {
    const e = this.editor;
    this.closeQuickPick();
    const [background, foreground, border, input, match] = this.colors();
    const root = document.createElement('div');
    root.setAttribute('role', 'dialog');
    root.setAttribute('aria-label', label);
    root.style.cssText = `background:${background};color:${foreground};border:1px solid ${border};width:420px;` +
      'font:13px system-ui,sans-serif;padding:4px';
    const field = document.createElement('input');
    field.setAttribute('aria-label', label);
    field.style.cssText = `width:100%;box-sizing:border-box;background:${input};color:${foreground};border:1px solid ${border};padding:3px 4px;font:inherit`;
    const list = document.createElement('div');
    list.setAttribute('role', 'listbox');
    list.style.cssText = 'max-height:260px;overflow:auto;margin-top:4px';
    root.append(field, list);
    let shown = items;
    let selected = 0;
    const render = () => {
      list.replaceChildren(...shown.map((item, i) => {
        const row = document.createElement('div');
        row.setAttribute('role', 'option');
        row.setAttribute('aria-selected', String(i === selected));
        row.style.cssText = `padding:2px 6px;cursor:pointer;white-space:pre;display:flex;justify-content:space-between;background:${i === selected ? `${match}55` : 'transparent'}`;
        const l = document.createElement('span');
        l.textContent = item.label;
        const d = document.createElement('span');
        d.textContent = item.detail ?? '';
        d.style.opacity = '.6';
        row.append(l, d);
        row.addEventListener('mousedown', (ev) => { ev.preventDefault(); done(item); });
        return row;
      }));
      list.children[selected]?.scrollIntoView({ block: 'nearest' });
    };
    const done = (item) => {
      this.closeQuickPick();
      e.focus();
      if (item) pick(item);
    };
    field.addEventListener('input', () => {
      const q = field.value.trim().toLowerCase();
      shown = items.filter((it) => it.label.toLowerCase().includes(q));
      selected = 0;
      render();
    });
    field.addEventListener('keydown', (ev) => {
      if (ev.key === 'ArrowDown') { ev.preventDefault(); selected = Math.min(shown.length - 1, selected + 1); render(); }
      if (ev.key === 'ArrowUp') { ev.preventDefault(); selected = Math.max(0, selected - 1); render(); }
      if (ev.key === 'Enter') { ev.preventDefault(); done(shown[selected]); }
      if (ev.key === 'Escape') { ev.preventDefault(); done(null); }
    });
    field.addEventListener('blur', () => setTimeout(() => { if (root.isConnected && !root.contains(document.activeElement)) this.closeQuickPick(); }, 0));
    render();
    e.addOverlayWidget({ domNode: root, position: 'top-center' });
    this.quickPickRoot = root;
    field.focus();
  }

  closeQuickPick() {
    if (!this.quickPickRoot) return;
    this.editor.removeOverlayWidget({ domNode: this.quickPickRoot });
    this.quickPickRoot = null;
  }

  // ---- semantic tokens, inlay hints ----

  /** Asks the semantic token providers again and draws their styles over the syntax ones. */
  async refreshSemanticTokens() {
    const providers = providersFor('semanticTokens', this.model.getLanguage());
    if (!providers.length) {
      if (this.hadSemantic) this.editor.core.setSemanticTokens(new Uint32Array());
      this.hadSemantic = false;
      return;
    }
    const version = this.model.core.version();
    const list = await askAll('semanticTokens', this.model, this.token('semanticTokens'), 'provideSemanticTokens');
    if (!list || version !== this.model.core.version()) return;
    // Rust maps the style names and sorts, in one call.
    this.editor.core.setSemanticTokensNamed(Uint32Array.from(list.flatMap((t) => [t.start, t.end])), list.map((t) => t.style ?? '').join('\n'));
    this.hadSemantic = true;
    this.editor.schedule();
  }

  /** Asks the inlay hint providers again. */
  async refreshInlayHints() {
    const providers = providersFor('inlayHints', this.model.getLanguage());
    if (!providers.length && !this.hadHints) return;
    const version = this.model.core.version();
    const range = { start: 0, end: this.model.core.charCount() };
    const hints = await askAll('inlayHints', this.model, this.token('inlayHints'), 'provideInlayHints', range);
    if (!hints || version !== this.model.core.version()) return;
    this.editor.core.setInlayHints(Uint32Array.from(hints.map((h) => h.offset)), hints.map((h) => h.label?.label ?? h.label));
    this.hadHints = hints.length > 0;
    this.editor.schedule();
  }

  // ---- code lens ----

  /** Asks the code lens providers again and shows their titles above the lines. */
  async refreshCodeLens() {
    const e = this.editor;
    const lenses = await askAll('codeLens', this.model, this.token('codeLens'), 'provideCodeLenses');
    if (!lenses) return;
    for (const id of this.lensZones ?? []) e.removeViewZone(id);
    this.lensZones = [];
    const byLine = new Map();
    for (const lens of lenses) {
      if (!byLine.has(lens.line)) byLine.set(lens.line, []);
      byLine.get(lens.line).push(lens);
    }
    const lh = e.core.line_height();
    const [, foreground] = this.colors();
    for (const [line, lenses] of byLine) {
      const node = document.createElement('div');
      node.setAttribute('role', 'group');
      node.setAttribute('aria-label', 'Code lens');
      node.style.cssText = `font:12px system-ui,sans-serif;color:${foreground};opacity:.7;display:flex;gap:10px;align-items:end;` +
        `padding-left:${e.core.indentOfLine(line)}px`;
      for (const lens of lenses) {
        const a = document.createElement('span');
        a.textContent = lens.title;
        a.style.cursor = lens.command ? 'pointer' : 'default';
        if (lens.command) a.addEventListener('mousedown', (ev) => { ev.preventDefault(); lens.command(e); e.focus(); });
        node.append(a);
      }
      this.lensZones.push(e.addViewZone({ afterLine: line - 1, height: Math.round(lh * 0.9), domNode: node }));
    }
  }

  /** Escape: closes the topmost feature UI. True when something closed. */
  escape() {
    if (this.signature) { this.closeSignature(); return true; }
    if (this.hover) { this.hideHover(); return true; }
    return this.closePeek();
  }
}
