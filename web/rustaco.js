// DOM glue for the Rustaco editor. Rust draws and edits; this file forwards browser events.
import init, { Editor, Model, setBuiltinTheme, setThemeRules } from './pkg/rustaco_web.js';

const BLINK_MS = 530;
/** Parsing time per frame; the rest of a long parse continues on later frames. */
const PARSE_BUDGET_MS = 4;

let ready;
function load() {
  ready ??= init().then(() => document.fonts.ready);
  return ready;
}

/** Creates a text model that several editors can show. `language` is an id such as `"rust"`. */
export async function createModel(value = '', language = '') {
  await load();
  const model = new RustacoModel(value);
  if (language) await model.setLanguage(language);
  return model;
}

/** Language id for a file name and its first line, or `""`. */
export async function detectLanguage(path, firstLine = '') {
  await load();
  return Model.detectLanguage(path, firstLine);
}

/** Ids of every language the editor knows. */
export async function knownLanguages() {
  await load();
  return Model.knownLanguages();
}

const packModules = new Map();

/** Loads the language module for `pack` once; resolves to the module or null when absent. */
function loadPack(pack) {
  if (!packModules.has(pack)) {
    const url = new URL(`./pkg/syntax-${pack}/rustaco_syntax_wasm.js`, import.meta.url);
    packModules.set(pack, import(url.href).then(async (m) => { await m.default(); return m; }).catch(() => null));
  }
  return packModules.get(pack);
}

const editors = new Set();

/**
 * Sets the theme for every editor on the page: `"dark"`, `"light"`, `"high-contrast"`, or a
 * VS Code colour theme object (the parsed JSON of a `*-color-theme.json` file). From a VS Code
 * theme the editor reads `tokenColors`, the editor surface entries of `colors`
 * (`editor.background`, `editor.selectionBackground`, ...) and `type` for the defaults.
 */
export async function setTheme(theme) {
  await load();
  if (typeof theme === 'string') {
    if (!setBuiltinTheme(theme)) throw new Error(`unknown built-in theme: ${theme}`);
  } else {
    const selectors = [];
    const foregrounds = [];
    const flags = [];
    for (const rule of theme.tokenColors ?? []) {
      const settings = rule.settings ?? {};
      const scopes = Array.isArray(rule.scope) ? rule.scope : String(rule.scope ?? '').split(',');
      const fg = parseColor(settings.foreground);
      const fontStyle = settings.fontStyle;
      let f = 0;
      if (typeof fontStyle === 'string') {
        f = 1;
        if (fontStyle.includes('bold')) f |= 2;
        if (fontStyle.includes('italic')) f |= 4;
        if (fontStyle.includes('underline')) f |= 8;
      }
      for (const scope of scopes) {
        const sel = scope.trim();
        if (!sel) continue;
        selectors.push(sel);
        foregrounds.push(fg);
        flags.push(f);
      }
    }
    const type = String(theme.type ?? 'dark');
    const base = type.startsWith('hc') ? 'high-contrast' : type === 'light' ? 'light' : 'dark';
    const colorKeys = Object.keys(theme.colors ?? {});
    const colorValues = colorKeys.map((k) => parseColor(theme.colors[k]));
    setThemeRules(base, selectors.join('\n'), Uint32Array.from(foregrounds), Uint8Array.from(flags),
      colorKeys.join('\n'), Uint32Array.from(colorValues));
  }
  for (const e of editors) e.schedule();
}

/** `#rgb`, `#rrggbb` or `#rrggbbaa` to 0xRRGGBBAA; 0 when absent or invalid. */
function parseColor(c) {
  if (typeof c !== 'string' || c[0] !== '#') return 0;
  let hex = c.slice(1);
  if (hex.length === 3) hex = [...hex].map((x) => x + x).join('');
  if (hex.length === 6) hex += 'ff';
  return hex.length === 8 && /^[0-9a-f]+$/i.test(hex) ? parseInt(hex, 16) >>> 0 : 0;
}

/**
 * Creates an editor in `container`. Pass `model` to show an existing model
 * (for example in a split view), or `value` to start a new one.
 */
export async function createEditor(container, options = {}) {
  await load();
  return new RustacoEditor(container, options);
}

export class RustacoModel {
  constructor(value = '') {
    this.core = new Model(value);
    this.editors = new Set();
    this.listeners = new Set();
    this.version = this.core.version();
    this.language = '';
    this.highlighter = null;
    this.highlighterVersion = 0;
  }

  getValue() { return this.core.getValue(); }

  /**
   * Sets the language by id; `""` turns highlighting off. Loads the language module on first
   * use and resolves to false when the id is unknown or its module is not available.
   */
  async setLanguage(id) {
    this.language = id;
    this.highlighter?.free();
    this.highlighter = null;
    let ok = id === '';
    const pack = id ? Model.languagePack(id) : '';
    const module = pack ? await loadPack(pack) : null;
    if (module && this.language === id) {
      this.highlighter = new module.Highlighter(id, this.getValue());
      this.highlighterVersion = this.core.version();
      ok = true;
    }
    for (const e of this.editors) e.schedule();
    return ok;
  }

  getLanguage() { return this.language; }

  /**
   * Brings the language module up to date: sends new edits and parses for at most
   * `budgetMs`. Returns true when the module's tree matches the text.
   */
  updateHighlighter(budgetMs = PARSE_BUDGET_MS) {
    if (!this.highlighter) return true;
    this.syncHighlighter();
    return this.highlighter.parse(budgetMs);
  }

  /** Sends the edits made since the last call to the language module. */
  syncHighlighter() {
    if (!this.highlighter) return;
    const v = this.core.version();
    if (v === this.highlighterVersion) return;
    const batch = this.core.editsSince(this.highlighterVersion);
    try {
      if (batch.stale()) this.highlighter.reset(this.getValue());
      else this.highlighter.edit(batch.numbers(), batch.text());
    } catch {
      this.highlighter.reset(this.getValue());
    } finally {
      batch.free();
    }
    this.highlighterVersion = v;
  }

  /** Replaces the text and clears the undo history. */
  setValue(text) {
    this.core.setValue(text);
    for (const e of this.editors) e.core.scroll_to_top();
    this.changed();
  }

  /** Calls `fn` after every text change. Returns a function that removes the listener. */
  onDidChangeContent(fn) {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  /** Redraws every editor showing this model and fires listeners if the text changed. */
  changed() {
    for (const e of this.editors) e.schedule();
    const v = this.core.version();
    if (v !== this.version) {
      this.version = v;
      for (const fn of this.listeners) fn();
    }
  }
}

export class RustacoEditor {
  constructor(container, {
    value = '',
    language = '',
    model = null,
    fontFamily = '"Cascadia Code", Consolas, "Courier New", monospace',
    fontSize = 14,
  } = {}) {
    this.container = container;
    this.canvas = document.createElement('canvas');
    this.canvas.style.cssText = 'display:block;width:100%;height:100%;cursor:text';
    this.input = document.createElement('textarea');
    this.input.setAttribute('autocorrect', 'off');
    this.input.setAttribute('autocapitalize', 'off');
    this.input.setAttribute('spellcheck', 'false');
    this.input.style.cssText =
      'position:absolute;left:0;top:0;width:1px;height:1em;padding:0;border:0;margin:0;' +
      'opacity:0;resize:none;overflow:hidden;white-space:pre;outline:none;pointer-events:none';
    if (getComputedStyle(container).position === 'static') container.style.position = 'relative';
    container.append(this.canvas, this.input);

    this.model = model ?? new RustacoModel(value);
    this.ready = language ? this.model.setLanguage(language) : Promise.resolve(true);
    editors.add(this);
    this.core = new Editor(this.canvas, this.model.core, fontFamily, fontSize);
    this.model.editors.add(this);
    this.caretOn = true;
    this.focused = false;
    this.composing = false;
    this.frame = 0;

    this.#wire();
    new ResizeObserver(() => this.#resize()).observe(container);
    this.#resize();
  }

  getModel() { return this.model; }
  getValue() { return this.model.getValue(); }
  setValue(text) { this.model.setValue(text); }
  focus() { this.input.focus(); }

  /** Selections as `{ anchor, head }` char offsets, the primary selection first. */
  getSelections() {
    const flat = this.core.selections();
    const out = [];
    for (let i = 0; i < flat.length; i += 2) out.push({ anchor: flat[i], head: flat[i + 1] });
    return out;
  }

  /** Replaces the selections; the first one becomes primary. */
  setSelections(selections) {
    this.core.set_selections(Uint32Array.from(selections.flatMap((s) => [s.anchor, s.head])));
    this.schedule();
  }

  /** Calls `fn` after every text change, from this or any other editor on the model. */
  onDidChangeContent(fn) { return this.model.onDidChangeContent(fn); }

  /** Detaches the editor from its model and removes its DOM. */
  dispose() {
    this.model.editors.delete(this);
    editors.delete(this);
    clearInterval(this.blink);
    this.canvas.remove();
    this.input.remove();
    this.core.free();
  }

  #resize() {
    const r = this.container.getBoundingClientRect();
    this.core.resize(r.width, r.height, window.devicePixelRatio || 1);
    this.schedule();
  }

  #point(e) {
    const r = this.canvas.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }

  #wire() {
    const { canvas, input, core } = this;

    canvas.addEventListener('mousedown', (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      input.focus();
      core.mouse_down(...this.#point(e), e.detail, e.shiftKey, e.altKey);
      this.#changed();
      const move = (ev) => { if (core.mouse_move(...this.#point(ev))) this.#changed(); };
      const up = () => {
        core.mouse_up();
        window.removeEventListener('mousemove', move);
        window.removeEventListener('mouseup', up);
      };
      window.addEventListener('mousemove', move);
      window.addEventListener('mouseup', up);
    });

    canvas.addEventListener('wheel', (e) => {
      e.preventDefault();
      const unit = e.deltaMode === 1 ? core.line_height() : e.deltaMode === 2 ? canvas.clientHeight : 1;
      const [dx, dy] = e.shiftKey && !e.deltaX ? [e.deltaY, 0] : [e.deltaX, e.deltaY];
      core.wheel(dx * unit, dy * unit);
      this.schedule();
    }, { passive: false });

    input.addEventListener('keydown', (e) => {
      if (e.isComposing || e.keyCode === 229) return;
      if (core.key_down(e.key, e.ctrlKey, e.shiftKey, e.altKey, e.metaKey)) {
        e.preventDefault();
        this.#changed();
      }
    });
    input.addEventListener('input', () => {
      if (this.composing) return;
      const text = input.value;
      input.value = '';
      if (text) { core.insert_text(text); this.#changed(); }
    });
    input.addEventListener('compositionstart', () => { this.composing = true; });
    input.addEventListener('compositionend', (e) => {
      this.composing = false;
      input.value = '';
      if (e.data) { core.insert_text(e.data); this.#changed(); }
    });

    input.addEventListener('copy', (e) => { e.preventDefault(); e.clipboardData.setData('text/plain', core.copy()); });
    input.addEventListener('cut', (e) => {
      e.preventDefault();
      e.clipboardData.setData('text/plain', core.cut());
      this.#changed();
    });
    input.addEventListener('paste', (e) => {
      e.preventDefault();
      core.paste(e.clipboardData.getData('text/plain'));
      this.#changed();
    });

    input.addEventListener('focus', () => { this.focused = true; this.#restartBlink(); });
    input.addEventListener('blur', () => { this.focused = false; clearInterval(this.blink); this.schedule(); });
  }

  #restartBlink() {
    clearInterval(this.blink);
    this.caretOn = true;
    this.blink = setInterval(() => { this.caretOn = !this.caretOn; this.schedule(); }, BLINK_MS);
  }

  #changed() {
    if (this.focused) this.#restartBlink();
    this.model.changed();
  }

  schedule() {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      const { model, core } = this;
      if (model.highlighter) {
        const parsed = model.updateHighlighter();
        const [first, end] = core.visibleLines();
        core.setHighlights(first, end, model.highlighter.highlight(first, end));
        if (!parsed) this.schedule();
      } else {
        core.clearHighlights();
      }
      core.render(this.focused && this.caretOn);
      this.input.style.transform = `translate(${this.core.caret_x()}px, ${this.core.caret_y()}px)`;
    });
  }
}
