// DOM glue for the Rustaco editor. Rust draws and edits; this file forwards browser events.
import init, { Editor, Model } from './pkg/rustaco_web.js';

const BLINK_MS = 530;

let ready;
function load() {
  ready ??= init().then(() => document.fonts.ready);
  return ready;
}

/** Creates a text model that several editors can show. */
export async function createModel(value = '') {
  await load();
  return new RustacoModel(value);
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
  }

  getValue() { return this.core.getValue(); }

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
      this.core.render(this.focused && this.caretOn);
      this.input.style.transform = `translate(${this.core.caret_x()}px, ${this.core.caret_y()}px)`;
    });
  }
}
