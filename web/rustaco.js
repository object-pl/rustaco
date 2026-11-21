// DOM glue for the Rustaco editor. Rust draws and edits; this file forwards browser events.
import init, {
  Editor, Model, defineDecorationType as defineType, setBuiltinTheme, setThemeRules, widgetColors,
} from './pkg/rustaco_web.js';
import { SuggestController } from './suggest.js';
import { Features } from './features.js';
import { providersFor } from './languages.js';

export { languages, CompletionItemKind, SymbolKind, fuzzyScore } from './languages.js';

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
/** Editors by id, for routing worker replies. */
const editorsById = new Map();
let nextEditorId = 1;

let worker = null;
let workerFailed = false;
/** Worker-side models by id. */
const workerModels = new Map();
/** Replies awaited: request id -> resolve. */
const pending = new Map();
let nextRequest = 1;
let nextWorkerModel = 1;

/** The shared language worker, started on first use; null where workers are unavailable. */
function syntaxWorker() {
  if (worker || workerFailed) return worker;
  try {
    worker = new Worker(new URL('./rustaco-worker.js', import.meta.url), { type: 'module' });
  } catch {
    workerFailed = true;
    return null;
  }
  worker.onmessage = (e) => {
    const m = e.data;
    if (m.type === 'reply') {
      pending.get(m.req)?.(m);
      pending.delete(m.req);
    } else {
      workerModels.get(m.id)?.receive(m);
    }
  };
  worker.onerror = () => {
    // A worker that cannot start (blocked by CSP, missing file) fails every request; models
    // fall back to parsing on the main thread.
    workerFailed = true;
    worker = null;
    for (const resolve of pending.values()) resolve({ ok: false });
    pending.clear();
    for (const s of workerModels.values()) s.model.workerFailed();
  };
  return worker;
}

function ask(message) {
  const w = syntaxWorker();
  if (!w) return Promise.resolve({ ok: false });
  return new Promise((resolve) => {
    const req = nextRequest++;
    pending.set(req, resolve);
    w.postMessage({ ...message, req });
  });
}

/**
 * A model's side in the worker: its language module and, once searched, a copy of its text.
 * Edits go over as they are made; replies come back tagged with the model version.
 */
class WorkerLink {
  constructor(model) {
    this.model = model;
    this.id = nextWorkerModel++;
    this.sent = model.core.version();
    this.syntax = false;
    this.text = false;
    this.hasTree = false;
    workerModels.set(this.id, this);
  }

  /** Opens the language module for `language` (in `pack`); resolves to whether it loaded. */
  async openSyntax(language, pack) {
    this.sync();
    this.syntax = true;
    const r = await ask({ type: 'open', id: this.id, pack, language, text: this.model.getValue(), version: this.sent });
    this.hasTree = !!r.hasTree;
    if (!r.ok) this.closeSyntax();
    return !!r.ok;
  }

  closeSyntax() {
    if (this.syntax) worker?.postMessage({ type: 'closeSyntax', id: this.id });
    this.syntax = false;
    this.hasTree = false;
  }

  /** Sends the edits made since the last call. */
  sync() {
    const v = this.model.core.version();
    if (v === this.sent || !worker) return;
    if (this.syntax || this.text) {
      const batch = this.model.core.editsSince(this.sent);
      if (batch.stale()) worker.postMessage({ type: 'edit', id: this.id, version: v, reset: this.model.getValue() });
      else worker.postMessage({ type: 'edit', id: this.id, version: v, numbers: batch.numbers(), text: batch.text() });
      batch.free();
    }
    this.sent = v;
  }

  frame(editor, runs, colorize, caret) {
    this.sync();
    worker?.postMessage({ type: 'frame', id: this.id, editor: editor.id, runs, colorize, caret });
  }

  async enclosing(start, end) {
    this.sync();
    return (await ask({ type: 'enclosing', id: this.id, start, end })).result ?? [];
  }

  async folding() {
    this.sync();
    const r = await ask({ type: 'folding', id: this.id });
    return { ranges: r.result ?? [], version: r.version };
  }

  /** Every match, as pairs of char offsets (null for an invalid pattern), with the version. */
  async find(pattern, { regex = false, caseSensitive = false, wholeWord = false } = {}) {
    this.sync();
    if (!this.text && worker) {
      worker.postMessage({ type: 'openText', id: this.id, text: this.model.getValue(), version: this.sent });
      this.text = true;
    }
    const r = await ask({ type: 'find', id: this.id, pattern, regex, caseSensitive, wholeWord });
    return { matches: r.result, version: r.version };
  }

  receive(m) {
    if (m.type === 'frame') {
      editorsById.get(m.editor)?.applyFrame(m);
    } else if (m.type === 'parsed') {
      // The background parse caught up: frames get their final colours.
      for (const e of this.model.editors) { e.frameKey = null; e.schedule(); }
    } else if (m.type === 'resync') {
      worker?.postMessage({ type: 'edit', id: this.id, version: this.model.core.version(), reset: this.model.getValue() });
      this.sent = this.model.core.version();
    }
  }

  close() {
    worker?.postMessage({ type: 'close', id: this.id });
    workerModels.delete(this.id);
  }
}

/**
 * Whether new models parse in a worker (the default where workers are available) or on
 * the main thread in time slices.
 */
let workersByDefault = true;
export function setWorkersEnabled(on) { workersByDefault = !!on; }


/** Clipboard type of the per-cursor pieces of a multi-cursor copy. */
const CLIPBOARD_META = 'application/x-rustaco-editor';

/** Languages whose blocks are marked by indentation, so indentation adds fold regions. */
const INDENTATION_BLOCKS = new Set(['python', 'yaml']);

/** Physical keys whose unshifted symbol names them in keybindings. */
const SYMBOL_CODES = {
  Backslash: '\\', Slash: '/', BracketLeft: '[', BracketRight: ']', Semicolon: ';', Quote: "'",
  Comma: ',', Period: '.', Minus: '-', Equal: '=', Backquote: '`',
};

/**
 * The key name a keybinding matches. With Shift held a symbol key types a different char
 * (`|` for `\\` on US layouts), so it is named by its physical key, as VS Code does;
 * otherwise the produced char counts, which keeps bindings working on other layouts.
 */
function bindingKey(e) {
  return e.shiftKey && SYMBOL_CODES[e.code] ? SYMBOL_CODES[e.code] : e.key;
}

/** The keymap platform: Cmd-based bindings on Apple devices. */
function platform() {
  const p = navigator.userAgentData?.platform ?? navigator.platform ?? '';
  if (/mac|iphone|ipad/i.test(p)) return 'mac';
  return /linux/i.test(p) ? 'linux' : 'windows';
}

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
 * Defines how a kind of decoration draws and returns its type for
 * `model.createDecorationsCollection`. Every part is optional: `background` (CSS colour),
 * `underline` (`"solid"`, `"wavy"` or `"dotted"`) with `underlineColor`, `wholeLine` (the
 * background covers whole lines), `gutter` (a bar beside the line numbers) and
 * `overviewRuler` (a mark on the scrollbar). Types are shared by all editors.
 */
export async function defineDecorationType({
  background = '', underline = '', underlineColor = '#ff0000', wholeLine = false, gutter = '', overviewRuler = '',
} = {}) {
  await load();
  return defineType(background, underline, underlineColor, wholeLine, gutter, overviewRuler);
}

/** Decoration owners from here up belong to collections; lower ids are editors' own. */
let nextDecorationOwner = 1 << 20;

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
    /** The model's side in the worker, when one runs (see `workerLink`). */
    this.link = null;
    /** `link` while the language module runs in the worker. */
    this.syntax = null;
    this.useWorker = workersByDefault;
  }

  /** The model's side in the worker, created on first use; null without workers. */
  workerLink() {
    if (!this.useWorker || !syntaxWorker()) return null;
    this.link ??= new WorkerLink(this);
    return this.link;
  }

  /** Moves the language module into the worker or back to the main thread. */
  async setWorkerEnabled(on) {
    this.useWorker = !!on;
    return this.setLanguage(this.language);
  }

  /** Called when the worker failed: parse on the main thread instead. */
  workerFailed() {
    this.link = null;
    if (this.syntax) this.setLanguage(this.language);
  }

  getValue() { return this.core.getValue(); }

  /** The text between two char offsets. */
  getTextRange(start, end) { return this.core.getTextRange(start, end); }

  /** `{ line, column }` (0-based, chars) of an offset. */
  positionAt(offset) {
    const [line, column] = this.core.positionAt(offset);
    return { line, column };
  }

  /** The offset of `{ line, column }` (0-based), clamped into the text. */
  offsetAt({ line, column }) { return this.core.offsetAt(line, column); }

  getLineCount() { return this.core.lineCount(); }

  /** Symbols from the syntax tree; empty without a tree. */
  async builtinSymbols() { return []; }

  /** The text of a line (0-based), without its line break. */
  getLineContent(line) {
    const start = this.core.offsetAt(line, 0);
    return this.core.getTextRange(start, this.core.offsetAt(line, 1 << 30));
  }

  /**
   * Sets the language by id; `""` turns highlighting off. Loads the language module on first
   * use and resolves to false when the id is unknown or its module is not available.
   */
  async setLanguage(id) {
    this.language = id;
    this.core.setLanguageConfig(id);
    this.highlighter?.free();
    this.highlighter = null;
    this.syntax?.closeSyntax();
    this.syntax = null;
    let ok = id === '';
    const pack = id ? Model.languagePack(id) : '';
    const link = pack ? this.workerLink() : null;
    if (link) {
      if (await link.openSyntax(id, pack) && this.language === id) {
        this.syntax = link;
        ok = true;
      } else {
        link.closeSyntax();
      }
    }
    const module = pack && !this.syntax ? await loadPack(pack) : null;
    if (module && this.language === id) {
      this.highlighter = new module.Highlighter(id, this.getValue());
      this.highlighterVersion = this.core.version();
      ok = true;
    }
    for (const e of this.editors) {
      e.refreshFoldingSoon();
      e.schedule();
    }
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

  /**
   * A set of decorations replaced together, like Monaco's decorations collection. Each
   * decoration is `{ start, end, type }`: char offsets and a type from
   * `defineDecorationType`. Decorations move with edits and show in every editor of the model.
   */
  createDecorationsCollection(decorations = []) {
    const owner = nextDecorationOwner++;
    const model = this;
    const collection = {
      set(list) {
        model.core.setDecorations(owner, Uint32Array.from(list.flatMap((d) => [d.start, d.end, d.type])));
        for (const e of model.editors) { e.core.refreshOverview(); e.schedule(); }
      },
      clear() { collection.set([]); },
    };
    if (decorations.length) collection.set(decorations);
    return collection;
  }

  /** Calls `fn` after every text change. Returns a function that removes the listener. */
  onDidChangeContent(fn) {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  /** Redraws every editor showing this model and fires listeners if the text changed. */
  changed() {
    for (const e of this.editors) e.schedule();
    const v0 = this.version;
    if (this.core.version() !== v0) {
      for (const e of this.editors) {
        e.refreshFindSoon();
        e.refreshFoldingSoon();
      }
    }
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
    keymap = 'default',
    rulers = [],
    renderWhitespace = 'none',
    guides = true,
    occurrencesHighlight = true,
    bracketPairColorization = true,
    wordWrap = 'off',
    minimap = { enabled: false },
    stickyScroll = { enabled: false },
    fontFamily = '"Cascadia Code", Consolas, "Courier New", monospace',
    fontSize = 14,
    ariaLabel = 'Editor content',
  } = {}) {
    this.container = container;
    this.canvas = document.createElement('canvas');
    this.canvas.style.cssText = 'display:block;width:100%;height:100%;cursor:text';
    this.input = document.createElement('textarea');
    this.input.setAttribute('autocorrect', 'off');
    this.input.setAttribute('autocapitalize', 'off');
    this.input.setAttribute('spellcheck', 'false');
    // Screen readers read the textarea: it holds the caret's line and selection (see #mirror).
    this.input.setAttribute('aria-label', ariaLabel);
    this.input.setAttribute('aria-roledescription', 'editor');
    this.input.setAttribute('aria-multiline', 'true');
    this.input.style.cssText =
      'position:absolute;left:0;top:0;width:1px;height:1em;padding:0;border:0;margin:0;' +
      'opacity:0;resize:none;overflow:hidden;white-space:pre;outline:none;pointer-events:none';
    if (getComputedStyle(container).position === 'static') container.style.position = 'relative';
    // DOM nodes of view zones and widgets go here, over the canvas.
    this.overlay = document.createElement('div');
    this.overlay.style.cssText = 'position:absolute;inset:0;overflow:hidden;pointer-events:none';
    container.append(this.canvas, this.input, this.overlay);
    this.zones = new Map();
    this.contentWidgets = new Set();
    this.nextZoneId = 1;

    this.model = model ?? new RustacoModel(value);
    this.ready = language ? this.model.setLanguage(language) : Promise.resolve(true);
    editors.add(this);
    this.id = nextEditorId++;
    editorsById.set(this.id, this);
    this.core = new Editor(this.canvas, this.model.core, fontFamily, fontSize);
    this.setKeymap(keymap);
    this.options = {};
    this.setOptions({ rulers, renderWhitespace, guides, occurrencesHighlight, bracketPairColorization, wordWrap, minimap, stickyScroll });
    this.refreshFolding();
    this.model.editors.add(this);
    this.caretOn = true;
    this.focused = false;
    this.composing = false;
    this.frame = 0;
    this.findRequest = 0;
    this.suggest = new SuggestController(this, widgetColors);
    this.features = new Features(this, widgetColors);

    this.#wire();
    new ResizeObserver(() => this.#resize()).observe(container);
    this.#resize();
  }

  getModel() { return this.model; }

  /**
   * Display options, Monaco's names: `rulers` (columns), `renderWhitespace` (`"none"` or
   * `"all"`), `guides` (indentation guides), `occurrencesHighlight`, `bracketPairColorization`,
   * `wordWrap` (`"off"` or `"on"`, wrapping at the editor's width), `minimap` (`{ enabled }`),
   * `stickyScroll` (`{ enabled, maxLineCount }`).
   */
  setOptions(options) {
    Object.assign(this.options, options);
    this.core.refreshOverview();
    const o = this.options;
    this.core.setViewOptions(
      Uint32Array.from(o.rulers ?? []), o.renderWhitespace === 'all', !!o.guides, !!o.occurrencesHighlight && !this.occurrencesFromProvider,
    );
    this.core.setWordWrap(o.wordWrap === 'on');
    this.core.setMinimap(!!(typeof o.minimap === 'object' ? o.minimap?.enabled : o.minimap));
    const sticky = o.stickyScroll ?? {};
    this.core.setStickyScroll(sticky.enabled ? (sticky.maxLineCount ?? 5) : 0);
    this.schedule();
  }

  /** Selects the keymap preset: `"default"` (VS Code) or `"jetbrains"`. */
  setKeymap(preset) {
    if (!this.core.setKeymap(preset, platform())) throw new Error(`unknown keymap: ${preset}`);
  }

  /**
   * Adds keybindings over the current keymap, in VS Code's format:
   * `[{ key: 'ctrl+k ctrl+c', command: 'editor.action.commentLine', when: 'editorHasSelection' }]`.
   * A command starting with `-` removes that binding.
   */
  addKeybindings(bindings) {
    this.core.addKeybindings(
      bindings.map((b) => b.key).join('\n'),
      bindings.map((b) => b.command).join('\n'),
      bindings.map((b) => b.when ?? '').join('\n'),
    );
  }

  /**
   * Searches and highlights every match. Options: `regex`, `caseSensitive`, `wholeWord`.
   * Returns the match count, or -1 for an invalid regular expression. Matches are kept up to
   * date while the text changes, until `clearFind()`.
   */
  find(pattern, { regex = false, caseSensitive = false, wholeWord = false } = {}) {
    this.findQuery = { pattern, options: { regex, caseSensitive, wholeWord } };
    const count = this.core.find(pattern, regex, caseSensitive, wholeWord);
    this.findActive = count >= 0 && pattern !== '';
    this.schedule();
    return count;
  }

  /** Selects the next match after the cursor (wrapping); `findPrevious` goes back. */
  findNext() { const ok = this.core.findNext(true); this.schedule(); return ok; }

  findPrevious() { const ok = this.core.findNext(false); this.schedule(); return ok; }

  /** Replaces the selected match and selects the next one. `$1` refers to regex groups. */
  replace(replacement) {
    const ok = this.core.replaceOne(replacement);
    this.#changed();
    return ok;
  }

  /** Replaces every match as one undo step; returns the number of replacements. */
  replaceAll(replacement) {
    const n = this.core.replaceAll(replacement);
    this.#changed();
    return n;
  }

  /** Puts a cursor on every match. */
  selectAllMatches() { const n = this.core.selectAllMatches(); this.schedule(); return n; }

  clearFind() {
    this.findActive = false;
    this.core.clearFind();
    this.schedule();
  }

  /** Re-runs the active search shortly after edits, once typing pauses. */
  refreshFindSoon() {
    if (!this.findActive) return;
    clearTimeout(this.findTimer);
    this.findTimer = setTimeout(() => {
      const link = this.model.workerLink();
      if (link && this.findQuery) {
        // On a large file the search takes long; the worker runs it while typing goes on.
        this.findAsync(this.findQuery.pattern, this.findQuery.options);
        return;
      }
      this.core.refreshFind();
      this.findWidget?.updateCount();
      this.schedule();
    }, 100);
  }

  /**
   * Like `find`, but the search runs in the worker where there is one, so the page stays
   * responsive on large files. Resolves to the match count, or -1 for an invalid pattern.
   */
  async findAsync(pattern, options = {}) {
    const link = this.model.workerLink();
    if (!link) return this.find(pattern, options);
    const { regex = false, caseSensitive = false, wholeWord = false } = options;
    if (this.core.setFindQuery(pattern, regex, caseSensitive, wholeWord) < 0) {
      this.findActive = false;
      this.schedule();
      return -1;
    }
    this.findActive = pattern !== '';
    this.findQuery = { pattern, options };
    const asked = ++this.findRequest;
    const { matches, version } = await link.find(pattern, options);
    if (asked !== this.findRequest || !this.findActive) return -1;
    if (version !== this.model.core.version()) {
      // The text changed while the worker searched: search again.
      this.refreshFindSoon();
      return matches?.length ? matches.length / 2 : 0;
    }
    const n = this.core.setFindMatches(Uint32Array.from(matches ?? []));
    this.findWidget?.updateCount();
    this.schedule();
    return n;
  }

  /** Recomputes foldable regions once typing pauses; the scan reads the whole file. */
  refreshFoldingSoon() {
    clearTimeout(this.foldTimer);
    this.foldTimer = setTimeout(() => {
      this.refreshFolding();
      this.core.refreshOverview();
      this.schedule();
    }, 300);
  }

  /**
   * Foldable regions from the language module's tree when it has one (bracket pairs,
   * comments, markdown sections), from indentation otherwise. Languages whose blocks are
   * indentation get both.
   */
  refreshFolding() {
    const folders = providersFor('foldingRange', this.model.getLanguage());
    if (folders.length) {
      Promise.all(folders.map(async (f) => { try { return (await f.provideFoldingRanges(this.model)) ?? []; } catch { return []; } }))
        .then((lists) => {
          this.core.setFoldingRanges(Uint32Array.from(lists.flat().flatMap((r) => [r.start, r.end])), false);
          this.schedule();
        });
      return;
    }
    const s = this.model.syntax;
    if (s) {
      if (!s.hasTree) { this.core.refreshFoldingRanges(); return; }
      s.folding().then(({ ranges, version }) => {
        if (version !== this.model.core.version()) { this.refreshFoldingSoon(); return; }
        this.core.setFoldingRanges(Uint32Array.from(ranges), INDENTATION_BLOCKS.has(this.model.language));
        this.schedule();
      });
      return;
    }
    const h = this.model.highlighter;
    if (h && h.hasTree()) {
      this.model.updateHighlighter(20);
      const lang = this.model.language;
      this.core.setFoldingRanges(Uint32Array.from(h.foldingRanges()), INDENTATION_BLOCKS.has(lang));
    } else {
      this.core.refreshFoldingRanges();
    }
  }

  /**
   * Opens empty space after line `afterLine` (0-based; -1 for above the first line),
   * `height` pixels tall, and shows `domNode` in it. The zone moves with edits. Returns
   * an id for `removeViewZone`.
   */
  addViewZone({ afterLine, height, domNode }) {
    const id = this.nextZoneId++;
    domNode.style.position = 'absolute';
    domNode.style.pointerEvents = 'auto';
    domNode.style.display = 'none';
    this.overlay.append(domNode);
    this.zones.set(id, domNode);
    this.core.addViewZone(id, afterLine, height);
    this.schedule();
    return id;
  }

  removeViewZone(id) {
    this.zones.get(id)?.remove();
    this.zones.delete(id);
    this.core.removeViewZone(id);
    this.schedule();
  }

  /**
   * Shows `domNode` next to the text at char `position`: below that line, or above it with
   * `preference: 'above'`. Hidden while the position is scrolled out of view. Call
   * `layoutContentWidget` after changing the widget's position.
   */
  addContentWidget(widget) {
    widget.domNode.style.position = 'absolute';
    widget.domNode.style.pointerEvents = 'auto';
    this.overlay.append(widget.domNode);
    this.contentWidgets.add(widget);
    this.schedule();
  }

  layoutContentWidget() { this.schedule(); }

  removeContentWidget(widget) {
    widget.domNode.remove();
    this.contentWidgets.delete(widget);
  }

  /**
   * Shows `domNode` over the editor at a fixed place: `'top-right'`, `'bottom-right'` or
   * `'top-center'` (a find bar, for example).
   */
  addOverlayWidget({ domNode, position = 'top-right' }) {
    const place = {
      'top-right': 'top:0;right:16px',
      'bottom-right': 'bottom:0;right:16px',
      'top-center': 'top:0;left:50%;transform:translateX(-50%)',
    }[position] ?? 'top:0;right:16px';
    domNode.style.cssText += `;position:absolute;pointer-events:auto;${place}`;
    this.overlay.append(domNode);
  }

  removeOverlayWidget({ domNode }) { domNode.remove(); }

  /** Places view zone nodes and content widgets after a frame. */
  #layoutOverlays() {
    if (this.zones.size) {
      const shown = new Set();
      const r = this.core.viewZoneRects();
      for (let i = 0; i < r.length; i += 5) {
        const node = this.zones.get(r[i]);
        if (!node) continue;
        shown.add(r[i]);
        Object.assign(node.style, {
          display: '', left: `${r[i + 1]}px`, top: `${r[i + 2]}px`, width: `${r[i + 3]}px`, height: `${r[i + 4]}px`,
        });
      }
      for (const [id, node] of this.zones) if (!shown.has(id)) node.style.display = 'none';
    }
    for (const w of this.contentWidgets) {
      const p = this.core.positionOf(w.position);
      if (p.length === 0) { w.domNode.style.display = 'none'; continue; }
      const above = w.preference === 'above';
      w.domNode.style.display = '';
      w.domNode.style.left = `${p[0]}px`;
      w.domNode.style.top = above ? `${p[1] - w.domNode.offsetHeight}px` : `${p[1] + p[2]}px`;
    }
  }

  getScrollTop() { return this.core.scrollTop(); }

  setScrollTop(top) {
    this.core.setScrollTop(top);
    this.schedule();
  }

  /** Runs a command by id (`editor.action.commentLine`, `cursorWordLeft`, ...). */
  runCommand(id) {
    const ok = this.core.runCommand(id);
    if (ok) this.#changed();
    return ok;
  }
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
    this.features.caretMoved();
    this.#mirror();
    this.schedule();
  }

  /** Calls `fn` after every text change, from this or any other editor on the model. */
  onDidChangeContent(fn) { return this.model.onDidChangeContent(fn); }

  /** Detaches the editor from its model and removes its DOM. */
  dispose() {
    this.model.editors.delete(this);
    editors.delete(this);
    editorsById.delete(this.id);
    clearInterval(this.blink);
    this.canvas.remove();
    this.input.remove();
    this.overlay.remove();
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
      this.#coverInput(null);
      this.suggest.close();
      const [x, y] = this.#point(e);
      if (e.button === 2 || (e.button === 1 && platform() === 'linux')) {
        // Right click: keep a selection under the pointer, else move the caret there. Then
        // the textarea goes under the pointer, so the browser's own menu (cut, copy, paste)
        // or Linux middle-click paste acts on it, and its events reach the editor.
        if (e.button === 1 || !core.insideSelection(x, y)) {
          core.mouse_down(x, y, 1, false, false);
          core.mouse_up();
        }
        this.#changed();
        this.#coverInput([x, y]);
        input.focus();
        return;
      }
      if (e.button !== 0) return;
      e.preventDefault();
      input.focus();
      this.features.hideHover();
      const definitionKey = platform() === 'mac' ? e.metaKey : e.ctrlKey;
      if (definitionKey && !e.altKey && !e.shiftKey && this.features.has('definition')) {
        const offset = core.textOffsetAt(x, y);
        if (offset >= 0) { this.features.goToDefinition(offset); return; }
      }
      core.mouse_down(x, y, e.detail, e.shiftKey, e.altKey);
      this.#changed();
      let last = [x, y];
      let timer = 0;
      const step = () => {
        // Past the top or bottom edge the view scrolls, faster the further out the pointer is.
        const h = canvas.clientHeight;
        const over = last[1] < 0 ? last[1] : last[1] > h ? last[1] - h : 0;
        if (!over) { clearInterval(timer); timer = 0; return; }
        core.wheel(0, Math.sign(over) * Math.min(Math.abs(over), 200) / 2);
        core.mouse_move(...last);
        this.#changed();
      };
      const move = (ev) => {
        last = this.#point(ev);
        if (core.mouse_move(...last)) this.#changed();
        const h = canvas.clientHeight;
        if ((last[1] < 0 || last[1] > h) && !timer) timer = setInterval(step, 16);
      };
      const up = (ev) => {
        clearInterval(timer);
        core.mouseUpWith(platform() === 'mac' ? ev.altKey : ev.ctrlKey);
        this.#changed();
        window.removeEventListener('mousemove', move);
        window.removeEventListener('mouseup', up);
      };
      window.addEventListener('mousemove', move);
      window.addEventListener('mouseup', up);
    });
    input.addEventListener('contextmenu', () => setTimeout(() => this.#coverInput(null), 0));
    input.addEventListener('paste', () => setTimeout(() => this.#coverInput(null), 0));
    input.addEventListener('blur', () => { this.#coverInput(null); this.suggest.close(); });

    canvas.addEventListener('mousemove', (e) => {
      if (e.buttons === 0) this.features.mouseMove(...this.#point(e));
    });
    canvas.addEventListener('mouseleave', () => this.features.mouseLeave());

    canvas.addEventListener('wheel', (e) => {
      e.preventDefault();
      const unit = e.deltaMode === 1 ? core.line_height() : e.deltaMode === 2 ? canvas.clientHeight : 1;
      const [dx, dy] = e.shiftKey && !e.deltaX ? [e.deltaY, 0] : [e.deltaX, e.deltaY];
      core.wheel(dx * unit, dy * unit);
      this.schedule();
    }, { passive: false });

    input.addEventListener('keydown', (e) => {
      if (e.isComposing || e.keyCode === 229) return;
      if (this.suggest.handleKey(e)) { e.preventDefault(); return; }
      if (e.key === 'Escape' && this.features.escape()) { e.preventDefault(); return; }
      const result = core.key_down(bindingKey(e), e.ctrlKey, e.shiftKey, e.altKey, e.metaKey);
      if (result === 0) return;
      e.preventDefault();
      if (result === 2) this.#frontendCommand(core.takeFrontendCommand());
      this.afterEdit();
    });
    // Text arrives through beforeinput, which can be cancelled, so the textarea keeps the
    // mirrored line and the model stays the only copy of the text.
    input.addEventListener('beforeinput', (e) => {
      if (this.composing || e.isComposing) return;
      const commands = {
        deleteContentBackward: 'deleteLeft',
        deleteContentForward: 'deleteRight',
        deleteWordBackward: 'deleteWordLeft',
        deleteWordForward: 'deleteWordRight',
        historyUndo: 'undo',
        historyRedo: 'redo',
      };
      if (e.inputType === 'insertText' || e.inputType === 'insertReplacementText') {
        e.preventDefault();
        const text = e.data ?? e.dataTransfer?.getData('text/plain') ?? '';
        if (text) {
          core.insert_text(text, this.#inLiteral());
          this.#changed();
          this.suggest.typed(text);
          this.features.typed(text);
        }
      } else if (e.inputType === 'insertLineBreak' || e.inputType === 'insertParagraph') {
        e.preventDefault();
        core.key_down('Enter', false, false, false, false);
        this.#changed();
      } else if (commands[e.inputType]) {
        e.preventDefault();
        core.runCommand(commands[e.inputType]);
        this.#changed();
      } else if (e.inputType !== 'insertFromPaste' && e.inputType !== 'insertFromDrop' && e.inputType !== 'deleteByCut') {
        // Formatting and other edits a plain textarea would apply have no meaning here.
        e.preventDefault();
      }
    });
    // Where beforeinput could not be cancelled, the textarea changed: apply the difference.
    input.addEventListener('input', () => {
      if (this.composing) return;
      const before = this.mirrored ?? '';
      const after = input.value;
      if (after === before) return;
      let p = 0;
      while (p < before.length && p < after.length && before[p] === after[p]) p++;
      let q = 0;
      while (q < before.length - p && q < after.length - p && before[before.length - 1 - q] === after[after.length - 1 - q]) q++;
      const inserted = after.slice(p, after.length - q);
      const removed = before.length - p - q;
      if (inserted) core.insert_text(inserted, this.#inLiteral());
      else if (removed > 0) core.runCommand('deleteLeft');
      this.#changed();
    });
    // IME: each step replaces the previous step's text in the model, underlined, so the
    // composition shows inline where it will land.
    input.addEventListener('compositionstart', () => { this.composing = true; this.composed = 0; });
    input.addEventListener('compositionupdate', (e) => {
      core.compose(this.composed, e.data ?? '');
      this.composed = [...(e.data ?? '')].length;
      this.#changed();
    });
    input.addEventListener('compositionend', (e) => {
      core.compose(this.composed, e.data ?? '');
      core.endComposition();
      this.composing = false;
      this.composed = 0;
      this.#changed();
    });

    input.addEventListener('copy', (e) => {
      e.preventDefault();
      this.#writeClipboard(e.clipboardData, core.copy());
    });
    input.addEventListener('cut', (e) => {
      e.preventDefault();
      const pieces = core.selectionTexts();
      const html = core.copyHtml();
      this.#writeClipboard(e.clipboardData, core.cut(), pieces, html);
      this.#changed();
    });
    input.addEventListener('paste', (e) => {
      e.preventDefault();
      const text = e.clipboardData.getData('text/plain');
      let meta = null;
      try { meta = JSON.parse(e.clipboardData.getData(CLIPBOARD_META) || 'null'); } catch { meta = null; }
      const cursors = core.selections().length / 2;
      if (meta?.pieces?.length > 1 && meta.pieces.length === cursors && meta.pieces.join('\n') === text.replace(/\r\n/g, '\n')) {
        core.pasteEach(meta.pieces);
      } else {
        core.paste(text);
      }
      this.#changed();
    });

    input.addEventListener('focus', () => { this.focused = true; this.#restartBlink(); });
    input.addEventListener('blur', () => { this.focused = false; clearInterval(this.blink); this.schedule(); });
  }

  /** Commands that need the language module or the DOM. */
  #frontendCommand(command) {
    if (this.features.run(command)) return;
    if (command === 'editor.action.triggerSuggest') {
      this.suggest.trigger({ explicit: true });
      return;
    }
    if (command === 'actions.find' || command === 'editor.action.startFindReplaceAction') {
      this.findWidget ??= new FindWidget(this);
      this.findWidget.open(command !== 'actions.find');
      return;
    }
    if (command === 'editor.action.jumpToBracket' && this.model.syntax) {
      // The pair at the caret came with the last frame.
      this.core.runCommand(command);
      return;
    }
    const selectors = providersFor('selectionRange', this.model.getLanguage());
    if (command === 'editor.action.smartSelect.expand' && selectors.length) {
      const head = this.core.selections()[1];
      Promise.resolve(selectors[0].provideSelectionRanges(this.model, head)).then((ranges) => {
        this.core.expandSelection(Uint32Array.from((ranges ?? []).flatMap((r) => [r.start, r.end])));
        this.#changed();
      });
      return;
    }
    if (command === 'editor.action.smartSelect.expand' && this.model.syntax) {
      const [start, end] = this.core.primaryRange();
      this.model.syntax.enclosing(start, end).then((ranges) => {
        this.core.expandSelection(Uint32Array.from(ranges));
        this.#changed();
      });
      return;
    }
    if (command === 'editor.action.jumpToBracket') {
      const h = this.model.highlighter;
      if (h && h.hasTree()) {
        this.model.updateHighlighter(50);
        this.core.setTreeBrackets(h.matchingBracket(this.core.primaryRange()[0]));
      } else {
        this.core.clearTreeBrackets();
      }
      this.core.runCommand(command);
      return;
    }
    if (command === 'editor.action.smartSelect.expand') {
      const h = this.model.highlighter;
      if (!h) return;
      this.model.updateHighlighter(50);
      const [start, end] = this.core.primaryRange();
      this.core.expandSelection(h.enclosingRanges(start, end));
    }
  }

  /** True when the primary cursor is inside a string or comment. */
  #inLiteral() {
    if (this.model.syntax) return this.core.caretInLiteral();
    const h = this.model.highlighter;
    if (!h) return false;
    this.model.updateHighlighter(2);
    return h.contextAt(this.core.primaryRange()[0]) !== 0;
  }

  #restartBlink() {
    clearInterval(this.blink);
    this.caretOn = true;
    this.blink = setInterval(() => { this.caretOn = !this.caretOn; this.schedule(); }, BLINK_MS);
  }

  #changed() {
    if (this.focused) this.#restartBlink();
    this.model.changed();
    this.#mirror();
    this.features?.caretMoved();
  }

  /**
   * After a command or an accepted completion: redraw, refilter an open list, and offer a
   * snippet stop's choices.
   */
  afterEdit() {
    this.#changed();
    const choices = this.core.snippetChoices();
    if (choices.length) this.suggest.showChoices(choices);
    else if (this.suggest.open) this.suggest.refilter();
    if (this.features.signature) this.features.signatureHelp(false);
  }

  /** Turns the built-in highlighting of the word at the caret off while a provider does it. */
  setOccurrencesFromProvider(fromProvider) {
    if (this.occurrencesFromProvider === fromProvider) return;
    this.occurrencesFromProvider = fromProvider;
    this.setOptions({});
  }

  /** The decoration type of a symbol's highlighted uses, defined on first use. */
  async highlightDecorationType() {
    this.highlightType ??= defineDecorationType({ background: `${widgetColors()[4]}55` });
    return this.highlightType;
  }

  /**
   * The document's symbols as a tree `[{ name, kind, range, selectionRange?, children? }]`:
   * from a symbol provider, else from the syntax tree.
   */
  async getDocumentSymbols() {
    for (const p of providersFor('documentSymbol', this.model.getLanguage())) {
      try {
        const r = await p.provideDocumentSymbols(this.model);
        if (r?.length) return r;
      } catch {
        // Try the next provider.
      }
    }
    return this.model.builtinSymbols();
  }

  /** Inserts a snippet in VS Code's syntax (`for ${1:i} in $2 {\n\t$0\n}`) at every cursor. */
  insertSnippet(source) {
    this.core.insertSnippet(source);
    this.afterEdit();
  }

  /** Opens the completion list, as Ctrl+Space does. */
  triggerSuggest() { return this.suggest.trigger({ explicit: true }); }

  /**
   * Puts the caret's line and selection into the hidden textarea, the way VS Code does, so a
   * screen reader reads the line the caret moves to and the selected text.
   */
  #mirror() {
    if (this.composing) return;
    const [text, start, end] = this.core.accessibleLine();
    if (this.input.value !== text) this.input.value = text;
    if (document.activeElement === this.input || this.container.getRootNode().activeElement === this.input) {
      this.input.setSelectionRange(start, end);
    }
    this.mirrored = text;
  }

  /**
   * Moves the hidden textarea under the pointer at `point` (canvas coordinates), large enough
   * to receive a context menu or a middle click; `null` puts it back behind the caret.
   */
  #coverInput(point) {
    const style = this.input.style;
    this.inputCovering = !!point;
    if (point) {
      style.transform = `translate(${point[0] - 15}px, ${point[1] - 15}px)`;
      style.width = '30px';
      style.height = '30px';
      style.pointerEvents = 'auto';
      style.zIndex = '10';
    } else {
      style.width = '1px';
      style.height = '1em';
      style.pointerEvents = 'none';
      style.zIndex = '';
      this.schedule();
    }
  }

  /** Clipboard data for a copy or cut: plain text, the pieces per cursor and coloured HTML. */
  #writeClipboard(data, text, pieces = this.core.selectionTexts(), html = this.core.copyHtml()) {
    data.setData('text/plain', text);
    if (pieces.length > 1) data.setData(CLIPBOARD_META, JSON.stringify({ pieces }));
    if (pieces.length) data.setData('text/html', html);
  }

  /** Takes a frame's highlights from the worker, unless the text has changed since. */
  applyFrame(m) {
    if (m.version !== this.model.core.version()) return;
    m.segments.forEach(([first, end, quads], i) => {
      if (i === 0) this.core.setHighlights(first, end, quads);
      else this.core.addHighlights(first, end, quads);
    });
    if (this.model.syntax?.hasTree) this.core.setTreeBrackets(m.bracket);
    else this.core.clearTreeBrackets();
    this.schedule();
  }

  schedule() {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      const { model, core } = this;
      const h = model.highlighter;
      const s = model.syntax;
      if (s) {
        // Ask the worker for this frame's highlights once per change of text, view or caret;
        // until they arrive the last ones (moved with the edits) are drawn.
        const runs = Array.from(core.visibleLines());
        const caret = core.primaryRange()[0];
        const colorize = !!this.options.bracketPairColorization;
        const key = `${model.core.version()}|${runs.join(',')}|${caret}|${colorize}`;
        if (key !== this.frameKey) {
          this.frameKey = key;
          s.frame(this, runs, colorize, caret);
        }
      } else if (h) {
        const parsed = model.updateHighlighter();
        // One run of lines per stretch between folds, so folded lines are never highlighted.
        const runs = core.visibleLines();
        const colorize = !!this.options.bracketPairColorization;
        for (let i = 0; i < runs.length; i += 2) {
          const quads = h.highlight(runs[i], runs[i + 1], colorize);
          if (i === 0) core.setHighlights(runs[i], runs[i + 1], quads);
          else core.addHighlights(runs[i], runs[i + 1], quads);
        }
        if (h.hasTree()) core.setTreeBrackets(h.matchingBracket(core.primaryRange()[0]));
        else core.clearTreeBrackets();
        if (!parsed) this.schedule();
      } else {
        core.clearHighlights();
        core.clearTreeBrackets();
      }
      core.render(this.focused && this.caretOn);
      if (!this.inputCovering) {
        this.input.style.transform = `translate(${this.core.caret_x()}px, ${this.core.caret_y()}px)`;
      }
      this.#layoutOverlays();
    });
  }
}

/**
 * The find and replace widget, an overlay at the top right: query with case, whole word and
 * regex toggles, "n of m", previous and next, and a replace row (Ctrl+H).
 */
class FindWidget {
  constructor(editor) {
    this.editor = editor;
    this.options = { caseSensitive: false, wholeWord: false, regex: false };
    const root = document.createElement('div');
    root.setAttribute('role', 'dialog');
    root.setAttribute('aria-label', 'Find');
    root.innerHTML = `
      <div data-row="find">
        <input data-field="find" aria-label="Find" placeholder="Find" spellcheck="false">
        <button data-toggle="caseSensitive" title="Match case (Alt+C)" aria-pressed="false">Aa</button>
        <button data-toggle="wholeWord" title="Match whole word (Alt+W)" aria-pressed="false">ab</button>
        <button data-toggle="regex" title="Use regular expression (Alt+R)" aria-pressed="false">.*</button>
        <span data-count aria-live="polite">No results</span>
        <button data-action="previous" title="Previous match (Shift+Enter)" aria-label="Previous match">&#x2191;</button>
        <button data-action="next" title="Next match (Enter)" aria-label="Next match">&#x2193;</button>
        <button data-action="close" title="Close (Escape)" aria-label="Close">&#x2715;</button>
      </div>
      <div data-row="replace">
        <input data-field="replace" aria-label="Replace" placeholder="Replace" spellcheck="false">
        <button data-action="replace" title="Replace (Enter)">Replace</button>
        <button data-action="replaceAll" title="Replace all (Ctrl+Alt+Enter)">All</button>
      </div>`;
    this.root = root;
    this.findInput = root.querySelector('[data-field="find"]');
    this.replaceInput = root.querySelector('[data-field="replace"]');
    this.replaceRow = root.querySelector('[data-row="replace"]');
    this.count = root.querySelector('[data-count]');
    this.#style();

    root.addEventListener('mousedown', (e) => e.stopPropagation());
    this.findInput.addEventListener('input', () => this.search(true));
    root.addEventListener('click', (e) => {
      const button = e.target.closest('button');
      if (!button) return;
      if (button.dataset.toggle) this.toggle(button.dataset.toggle);
      const action = button.dataset.action;
      if (action === 'next') this.step(true);
      if (action === 'previous') this.step(false);
      if (action === 'close') this.close();
      if (action === 'replace') this.replace();
      if (action === 'replaceAll') this.replaceAll();
    });
    root.addEventListener('keydown', (e) => {
      const inReplace = e.target === this.replaceInput;
      if (e.key === 'Escape') { e.preventDefault(); this.close(); return; }
      if (e.altKey && !e.ctrlKey && ['c', 'w', 'r'].includes(e.key.toLowerCase())) {
        e.preventDefault();
        this.toggle({ c: 'caseSensitive', w: 'wholeWord', r: 'regex' }[e.key.toLowerCase()]);
        return;
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        if (inReplace) {
          if (e.ctrlKey && e.altKey) this.replaceAll();
          else this.replace();
        } else {
          this.step(!e.shiftKey);
        }
      }
      if (e.key === 'F3') { e.preventDefault(); this.step(!e.shiftKey); }
    });
  }

  #style() {
    const [background, foreground, border, input, match] = widgetColors();
    this.root.style.cssText =
      `background:${background};color:${foreground};border:1px solid ${border};padding:4px 6px;` +
      'font:12px system-ui,sans-serif;display:flex;flex-direction:column;gap:4px;min-width:360px;top:0;right:16px';
    for (const row of this.root.querySelectorAll('[data-row]')) row.style.cssText = 'display:flex;align-items:center;gap:2px';
    for (const field of this.root.querySelectorAll('input')) {
      field.style.cssText = `flex:1;min-width:0;background:${input};color:${foreground};border:1px solid ${border};` +
        'padding:3px 4px;font:inherit;outline:none';
    }
    for (const b of this.root.querySelectorAll('button')) {
      b.style.cssText = `background:transparent;color:${foreground};border:1px solid transparent;padding:2px 5px;` +
        'font:inherit;cursor:pointer;min-width:22px';
    }
    this.count.style.cssText = 'padding:0 6px;white-space:nowrap;min-width:64px';
    this.matchColor = match;
    for (const [name, on] of Object.entries(this.options)) this.#mark(name, on);
  }

  #mark(name, on) {
    const b = this.root.querySelector(`[data-toggle="${name}"]`);
    b.setAttribute('aria-pressed', String(on));
    b.style.borderColor = on ? this.matchColor : 'transparent';
  }

  open(withReplace) {
    const e = this.editor;
    if (!this.root.isConnected) e.addOverlayWidget({ domNode: this.root, position: 'top-right' });
    this.replaceRow.style.display = withReplace ? 'flex' : 'none';
    const seed = e.core.selectionSeed();
    if (seed) this.findInput.value = seed;
    (withReplace && this.findInput.value ? this.replaceInput : this.findInput).focus();
    this.findInput.select();
    this.search(false);
  }

  close() {
    this.editor.clearFind();
    this.editor.removeOverlayWidget({ domNode: this.root });
    this.editor.focus();
  }

  toggle(name) {
    this.options[name] = !this.options[name];
    this.#mark(name, this.options[name]);
    this.search(true);
  }

  /** Runs the query; `move` selects the first match at or after the selection's start. */
  async search(move) {
    const e = this.editor;
    const query = this.findInput.value;
    const n = await e.findAsync(query, this.options);
    if (query !== this.findInput.value) return;
    if (n > 0 && move) {
      const [start] = e.core.primaryRange();
      e.setSelections([{ anchor: start, head: start }]);
      e.findNext();
    }
    this.updateCount(n < 0);
  }

  step(forward) {
    if (forward) this.editor.findNext(); else this.editor.findPrevious();
    this.updateCount();
  }

  replace() {
    this.editor.replace(this.replaceInput.value);
    this.updateCount();
  }

  replaceAll() {
    this.editor.replaceAll(this.replaceInput.value);
    this.updateCount();
  }

  updateCount(invalid = false) {
    if (!this.root.isConnected) return;
    const [index, total] = this.editor.core.findPosition();
    this.count.textContent = invalid ? 'Invalid pattern'
      : total === 0 ? 'No results'
        : index ? `${index} of ${total}` : `? of ${total}`;
  }
}
