// The completion list: asks the completion providers, filters fuzzily as the word grows,
// shows a documentation pane for the selected item, and inserts plain text or snippets.

import { CompletionItemKind, fuzzyScore, providersFor } from './languages.js';

/** Icons for item kinds: one letter in a small box. */
const KIND_ICON = {
  [CompletionItemKind.Method]: 'm', [CompletionItemKind.Function]: 'f', [CompletionItemKind.Constructor]: 'c',
  [CompletionItemKind.Field]: 'F', [CompletionItemKind.Variable]: 'v', [CompletionItemKind.Class]: 'C',
  [CompletionItemKind.Struct]: 'S', [CompletionItemKind.Interface]: 'I', [CompletionItemKind.Module]: 'M',
  [CompletionItemKind.Property]: 'p', [CompletionItemKind.Keyword]: 'k', [CompletionItemKind.Snippet]: 's',
  [CompletionItemKind.Constant]: 'K', [CompletionItemKind.Enum]: 'E', [CompletionItemKind.EnumMember]: 'e',
  [CompletionItemKind.Text]: 't', [CompletionItemKind.TypeParameter]: 'T', [CompletionItemKind.Value]: 'V',
};

const VISIBLE_ROWS = 12;
const ROW_HEIGHT = 22;
/** Words collected for word completion: from this many lines around the caret. */
const WORD_LINES = 5000;

const isWordChar = (c) => /[\p{L}\p{N}_]/u.test(c);

/** The primary caret: the head of the primary selection. */
const caret = (editor) => editor.core.selections()[1];

export class SuggestController {
  constructor(editor, colors) {
    this.editor = editor;
    this.colors = colors;
    this.open = false;
    this.items = [];
    this.filtered = [];
    this.selected = 0;
    this.request = 0;
    this.#build();
  }

  #build() {
    const [background, foreground, border, , match] = this.colors();
    const root = document.createElement('div');
    root.setAttribute('role', 'listbox');
    root.setAttribute('aria-label', 'Suggestions');
    root.style.cssText = `display:flex;align-items:flex-start;font:13px system-ui,sans-serif;color:${foreground}`;
    this.list = document.createElement('div');
    this.list.style.cssText = `background:${background};border:1px solid ${border};min-width:260px;max-width:440px;` +
      `max-height:${VISIBLE_ROWS * ROW_HEIGHT}px;overflow-y:auto`;
    this.docs = document.createElement('div');
    this.docs.style.cssText = `background:${background};border:1px solid ${border};border-left:0;padding:6px 8px;` +
      'max-width:360px;max-height:280px;overflow:auto;white-space:pre-wrap;display:none';
    root.append(this.list, this.docs);
    root.addEventListener('mousedown', (e) => {
      e.preventDefault();
      const row = e.target.closest('[data-index]');
      if (row) { this.selected = Number(row.dataset.index); this.accept(); }
    });
    this.root = root;
    this.matchColor = match;
    this.widget = { domNode: root, position: 0, preference: 'below' };
  }

  /** Asks the providers and opens the list. `explicit` is Ctrl+Space. */
  async trigger({ explicit = false, character = null } = {}) {
    const e = this.editor;
    const model = e.getModel();
    const [ws] = e.core.wordAtCaret();
    const head = caret(e);
    const context = { triggerKind: explicit ? 'invoke' : character ? 'character' : 'automatic', triggerCharacter: character };
    const asked = ++this.request;
    const providers = providersFor('completion', model.getLanguage());
    const results = await Promise.all(providers.map(async (p) => {
      try {
        const r = await p.provideCompletionItems(model, head, context);
        return Array.isArray(r) ? r : r?.suggestions ?? [];
      } catch {
        return [];
      }
    }));
    if (asked !== this.request) return;
    let items = results.flat().map((item) => normalize(item, ws, head));
    if (items.length === 0) items = wordItems(model, ws, head);
    this.items = items;
    this.start = ws;
    this.explicit = explicit;
    this.refilter();
  }

  /** Shows the options of a snippet's choice stop. */
  showChoices(choices) {
    const [start, end] = this.editor.core.primaryRange();
    this.items = choices.map((label) => ({ label, kind: CompletionItemKind.Value, insertText: label, range: { start, end }, choice: true }));
    this.start = start;
    this.explicit = true;
    this.choiceMode = true;
    this.refilter();
  }

  /** Filters by the text from the word start to the caret; closes when nothing is left. */
  refilter() {
    const e = this.editor;
    const head = caret(e);
    if (head < this.start) { this.close(); return; }
    const prefix = this.choiceMode ? '' : e.getModel().getTextRange(this.start, head);
    if ([...prefix].some((c) => !isWordChar(c))) { this.close(); return; }
    const scored = [];
    for (const item of this.items) {
      const m = fuzzyScore(prefix, item.filterText ?? item.label);
      if (m) scored.push({ item, score: m.score, matches: (item.filterText ?? item.label) === item.label ? m.matches : [] });
    }
    // Choices keep the snippet's order; completions sort by score, then sort text.
    if (!this.choiceMode) {
      scored.sort((a, b) => b.score - a.score || (a.item.sortText ?? a.item.label).localeCompare(b.item.sortText ?? b.item.label));
    }
    // An exact match of the only item is already typed: nothing to offer.
    if (scored.length === 0 || (!this.explicit && scored.length === 1 && scored[0].item.label === prefix)) {
      this.close();
      return;
    }
    this.filtered = scored;
    this.selected = 0;
    this.#render();
    if (!this.open) {
      this.widget.position = this.start;
      e.addContentWidget(this.widget);
      this.open = true;
    } else {
      e.layoutContentWidget(this.widget);
    }
  }

  #render() {
    const [, foreground, , , match] = this.colors();
    this.list.replaceChildren(...this.filtered.map(({ item, matches }, i) => {
      const row = document.createElement('div');
      row.dataset.index = String(i);
      row.setAttribute('role', 'option');
      row.style.cssText = `display:flex;gap:6px;align-items:center;height:${ROW_HEIGHT}px;padding:0 6px;cursor:pointer;white-space:nowrap`;
      const icon = document.createElement('span');
      icon.textContent = KIND_ICON[item.kind] ?? 't';
      icon.style.cssText = `width:14px;text-align:center;opacity:.7;font-size:11px;border:1px solid ${foreground}40`;
      const label = document.createElement('span');
      label.style.cssText = 'flex:1;overflow:hidden;text-overflow:ellipsis';
      const chars = [...item.label];
      chars.forEach((c, ci) => {
        if (matches.includes(ci)) {
          const b = document.createElement('b');
          b.textContent = c;
          b.style.color = match;
          label.append(b);
        } else {
          label.append(c);
        }
      });
      const detail = document.createElement('span');
      detail.textContent = item.detail ?? '';
      detail.style.cssText = 'opacity:.6;font-size:12px;overflow:hidden;text-overflow:ellipsis;max-width:160px';
      row.append(icon, label, detail);
      return row;
    }));
    this.#highlight();
  }

  #highlight() {
    const [, , , input, match] = this.colors();
    [...this.list.children].forEach((row, i) => {
      const on = i === this.selected;
      row.style.background = on ? `${match}55` : 'transparent';
      row.setAttribute('aria-selected', String(on));
    });
    const row = this.list.children[this.selected];
    row?.scrollIntoView({ block: 'nearest' });
    const item = this.filtered[this.selected]?.item;
    const doc = item ? [item.detail, item.documentation?.value ?? item.documentation].filter(Boolean).join('\n\n') : '';
    this.docs.textContent = doc;
    this.docs.style.display = doc ? '' : 'none';
    void input;
  }

  move(delta) {
    const n = this.filtered.length;
    this.selected = Math.max(0, Math.min(n - 1, this.selected + delta));
    this.#highlight();
  }

  /** Inserts the selected item, replacing the typed prefix (or the item's range). */
  accept() {
    const entry = this.filtered[this.selected];
    if (!entry) return;
    const item = entry.item;
    const e = this.editor;
    const head = caret(e);
    const range = item.range ?? { start: this.start, end: head };
    const before = Math.max(0, head - range.start);
    const after = Math.max(0, range.end - head);
    this.close();
    e.core.acceptCompletion(before, after, item.insertText ?? item.label, !!item.isSnippet);
    e.afterEdit();
    item.command?.(e);
  }

  close() {
    if (!this.open) { this.choiceMode = false; return; }
    this.open = false;
    this.choiceMode = false;
    this.request++;
    this.editor.removeContentWidget(this.widget);
  }

  /** Keys while the list is open; true when the key was used. */
  handleKey(e) {
    if (!this.open) return false;
    switch (e.key) {
      case 'ArrowDown': this.move(1); return true;
      case 'ArrowUp': this.move(-1); return true;
      case 'PageDown': this.move(VISIBLE_ROWS); return true;
      case 'PageUp': this.move(-VISIBLE_ROWS); return true;
      case 'Enter':
      case 'Tab': this.accept(); return true;
      case 'Escape': this.close(); return true;
      default: return false;
    }
  }

  /** After text was typed: open on word chars and trigger characters, refilter when open. */
  typed(text) {
    const e = this.editor;
    const language = e.getModel().getLanguage();
    const triggers = providersFor('completion', language).flatMap((p) => p.triggerCharacters ?? []);
    const last = [...text].pop() ?? '';
    if (triggers.includes(last)) {
      this.trigger({ character: last });
    } else if (this.open) {
      this.refilter();
    } else if (isWordChar(last) && e.options.quickSuggestions !== false) {
      this.trigger();
    }
  }
}

function normalize(item, wordStart, head) {
  const label = typeof item.label === 'string' ? item.label : item.label?.label ?? '';
  return {
    ...item,
    label,
    insertText: item.insertText ?? label,
    isSnippet: !!(item.isSnippet || item.insertTextRules === 4),
    range: item.range ?? { start: wordStart, end: head },
  };
}

/** Words of the text around the caret, for languages without a completion provider. */
function wordItems(model, wordStart, head) {
  const [line] = model.core.positionAt(head);
  const from = model.core.offsetAt(Math.max(0, line - WORD_LINES), 0);
  const to = model.core.offsetAt(line + WORD_LINES, 1 << 30);
  const text = model.getTextRange(from, to);
  const current = model.getTextRange(wordStart, head);
  const words = new Set(text.match(/[\p{L}_][\p{L}\p{N}_]*/gu) ?? []);
  words.delete(current);
  return [...words].slice(0, 5000).map((w) => ({
    label: w, kind: CompletionItemKind.Text, insertText: w, range: { start: wordStart, end: head },
  }));
}
