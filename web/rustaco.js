// DOM glue for the Rustaco editor. Rust draws and edits; this file forwards browser events.
import init, { Editor } from './pkg/rustaco_web.js';

const BLINK_MS = 530;

export async function createEditor(container, options = {}) {
  await init();
  await document.fonts.ready;
  return new RustacoEditor(container, options);
}

export class RustacoEditor {
  constructor(container, {
    value = '',
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
      'opacity:0;resize:none;overflow:hidden;white-space:pre;outline:none';
    if (getComputedStyle(container).position === 'static') container.style.position = 'relative';
    container.append(this.canvas, this.input);

    this.core = new Editor(this.canvas, value, fontFamily, fontSize);
    this.listeners = new Set();
    this.version = this.core.version();
    this.caretOn = true;
    this.focused = false;
    this.composing = false;
    this.frame = 0;

    this.#wire();
    new ResizeObserver(() => this.#resize()).observe(container);
    this.#resize();
  }

  getValue() { return this.core.getValue(); }
  setValue(text) { this.core.setValue(text); this.#changed(); }
  focus() { this.input.focus(); }

  /** Calls `fn` after every text change. Returns a function that removes the listener. */
  onDidChangeContent(fn) {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  #resize() {
    const r = this.container.getBoundingClientRect();
    this.core.resize(r.width, r.height, window.devicePixelRatio || 1);
    this.#schedule();
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
      this.#schedule();
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
    input.addEventListener('blur', () => { this.focused = false; clearInterval(this.blink); this.#schedule(); });
  }

  #restartBlink() {
    clearInterval(this.blink);
    this.caretOn = true;
    this.blink = setInterval(() => { this.caretOn = !this.caretOn; this.#schedule(); }, BLINK_MS);
  }

  #changed() {
    if (this.focused) this.#restartBlink();
    this.#schedule();
    const v = this.core.version();
    if (v !== this.version) {
      this.version = v;
      for (const fn of this.listeners) fn();
    }
  }

  #schedule() {
    if (this.frame) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.core.render(this.focused && this.caretOn);
      this.input.style.transform = `translate(${this.core.caret_x()}px, ${this.core.caret_y()}px)`;
    });
  }
}
