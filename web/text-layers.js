// Text layers other than the 2D canvas. Rust decides what changes (rows to set or move,
// glyph instances and atlas entries); these classes only apply it to the DOM or to WebGL.
import { textLayerCss } from './pkg/rustaco_web.js';

const sheets = new Map();

/** The DOM layer's stylesheet for `root` (document or shadow root), refreshed on theme change. */
function sheetFor(root) {
  let sheet = sheets.get(root);
  if (!sheet) {
    sheet = new CSSStyleSheet();
    sheets.set(root, sheet);
    root.adoptedStyleSheets = [...root.adoptedStyleSheets, sheet];
  }
  return sheet;
}

/** Rewrites the class rules after a theme change. */
export function refreshTextLayerCss() {
  const css = textLayerCss();
  for (const [, sheet] of sheets) sheet.replaceSync(BASE_CSS + css);
}

const BASE_CSS = `
.rx-text{position:absolute;inset:0;overflow:hidden;pointer-events:none;contain:strict;white-space:pre}
.rx-text>div{position:absolute;left:0;top:0;contain:layout style;will-change:transform}
.rx-w1,.rx-w2{display:inline-block;overflow:visible}
.rx-w1{width:var(--rx-cw)}.rx-w2{width:calc(2*var(--rx-cw))}
`;

export class DomTextLayer {
  constructor(container, font, lineHeight, charWidth) {
    this.el = document.createElement('div');
    this.el.className = 'rx-text';
    this.el.setAttribute('aria-hidden', 'true');
    this.el.style.font = font;
    this.el.style.lineHeight = `${lineHeight}px`;
    this.el.style.setProperty('--rx-cw', `${charWidth}px`);
    this.rows = new Map();
    sheetFor(container.getRootNode()).replaceSync(BASE_CSS + textLayerCss());
  }

  apply(core) {
    const ops = core.domOps();
    if (!ops.length) return;
    const html = ops.some((_, i) => i % 4 === 0 && ops[i] === 1) ? core.domHtml().split('\0') : [];
    let next = 0;
    for (let i = 0; i < ops.length; i += 4) {
      const id = ops[i + 1];
      let row = this.rows.get(id);
      if (ops[i] === 2) {
        row?.remove();
        this.rows.delete(id);
        continue;
      }
      if (!row) {
        row = document.createElement('div');
        this.rows.set(id, row);
        this.el.append(row);
      }
      if (ops[i] === 1) row.innerHTML = html[next++];
      row.style.transform = `translate(${ops[i + 2]}px,${ops[i + 3]}px)`;
    }
  }

  dispose() { this.el.remove(); }
}

const VERTEX = `#version 300 es
layout(location = 0) in vec2 a_pos;
layout(location = 1) in uint a_slot;
layout(location = 2) in vec4 a_color;
uniform vec2 u_view;
uniform vec2 u_slot;
uniform float u_cols;
uniform float u_atlas;
out vec2 v_uv;
out vec4 v_color;
void main() {
  vec2 corner = vec2(float(gl_VertexID & 1), float(gl_VertexID >> 1));
  vec2 pos = a_pos + corner * u_slot;
  gl_Position = vec4(pos.x / u_view.x * 2.0 - 1.0, 1.0 - pos.y / u_view.y * 2.0, 0.0, 1.0);
  float s = float(a_slot);
  vec2 cell = vec2(mod(s, u_cols), floor(s / u_cols));
  v_uv = (cell + corner) * u_slot / u_atlas;
  v_color = a_color;
}`;

const FRAGMENT = `#version 300 es
precision highp float;
uniform sampler2D u_tex;
in vec2 v_uv;
in vec4 v_color;
out vec4 color;
void main() {
  float a = texture(u_tex, v_uv).a * v_color.a;
  color = vec4(v_color.rgb * a, a);
}`;

const VARIANTS = ['', 'italic ', 'bold ', 'italic bold '];

export class GlTextLayer {
  constructor(memory, fontFamily, fontSize) {
    this.memory = memory;
    this.fontFamily = fontFamily;
    this.fontSize = fontSize;
    this.el = document.createElement('canvas');
    this.el.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;pointer-events:none';
    const gl = this.el.getContext('webgl2', { alpha: true, premultipliedAlpha: true, antialias: false });
    if (!gl) throw new Error('WebGL2 is not available');
    this.gl = gl;
    const shader = (type, src) => {
      const s = gl.createShader(type);
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
      return s;
    };
    const p = gl.createProgram();
    gl.attachShader(p, shader(gl.VERTEX_SHADER, VERTEX));
    gl.attachShader(p, shader(gl.FRAGMENT_SHADER, FRAGMENT));
    gl.linkProgram(p);
    if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
    this.program = p;
    this.u = Object.fromEntries(['u_view', 'u_slot', 'u_cols', 'u_atlas', 'u_tex'].map((n) => [n, gl.getUniformLocation(p, n)]));
    this.vao = gl.createVertexArray();
    gl.bindVertexArray(this.vao);
    this.buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 16, 0);
    gl.vertexAttribDivisor(0, 1);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribIPointer(1, 1, gl.UNSIGNED_INT, 16, 8);
    gl.vertexAttribDivisor(1, 1);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribPointer(2, 4, gl.UNSIGNED_BYTE, true, 16, 12);
    gl.vertexAttribDivisor(2, 1);
    this.texture = gl.createTexture();
    this.atlasSide = 0;
    this.raster = document.createElement('canvas');
    this.rasterCtx = this.raster.getContext('2d', { willReadFrequently: true });
  }

  #ensureAtlas(side) {
    const { gl } = this;
    if (this.atlasSide === side) return;
    this.atlasSide = side;
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, side, side, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  }

  /** Draws the glyphs Rust has not seen before into their atlas slots. */
  #rasterize(core, [w, h, pad, cols, side, reset]) {
    const { gl } = this;
    this.#ensureAtlas(side);
    if (reset) {
      gl.bindTexture(gl.TEXTURE_2D, this.texture);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, side, side, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    }
    const pending = core.glPending();
    if (!pending.length) return;
    const chars = [...core.glPendingChars()];
    // The new slots are consecutive: draw the rows of slots they fill on a scratch canvas
    // and upload that band.
    const first = Math.floor(pending[0] / cols);
    const last = Math.floor(pending[pending.length - 3] / cols);
    const bandH = (last - first + 1) * h;
    const r = this.raster;
    if (r.width !== side || r.height < bandH) { r.width = side; r.height = Math.max(bandH, r.height); }
    const ctx = this.rasterCtx;
    ctx.clearRect(0, 0, side, bandH);
    ctx.fillStyle = '#fff';
    ctx.textBaseline = 'middle';
    const dpr = this.dpr;
    let variant = -1;
    for (let i = 0, g = 0; i < pending.length; i += 3, g++) {
      const slot = pending[i];
      if (pending[i + 1] !== variant) {
        variant = pending[i + 1];
        ctx.font = `${VARIANTS[variant]}${this.fontSize * dpr}px ${this.fontFamily}`;
      }
      const x = (slot % cols) * w + pad + pending[i + 2] / 4;
      const y = (Math.floor(slot / cols) - first) * h + h / 2;
      ctx.fillText(chars[g], x, y);
    }
    const data = ctx.getImageData(0, 0, side, bandH);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, first * h, side, bandH, gl.RGBA, gl.UNSIGNED_BYTE, data);
  }

  apply(core, dpr) {
    const { gl, el } = this;
    this.dpr = dpr;
    const geometry = core.glGeometry();
    if (!geometry.length) return;
    this.#rasterize(core, geometry);
    const [ptr, count] = core.glInstances();
    const width = Math.round(el.clientWidth * dpr);
    const height = Math.round(el.clientHeight * dpr);
    if (el.width !== width || el.height !== height) { el.width = width; el.height = height; }
    gl.viewport(0, 0, width, height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (!count) return;
    gl.useProgram(this.program);
    gl.bindVertexArray(this.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Uint8Array(this.memory.buffer, ptr, count * 16), gl.STREAM_DRAW);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.uniform1i(this.u.u_tex, 0);
    gl.uniform2f(this.u.u_view, width, height);
    gl.uniform2f(this.u.u_slot, geometry[0], geometry[1]);
    gl.uniform1f(this.u.u_cols, geometry[3]);
    gl.uniform1f(this.u.u_atlas, geometry[4]);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, count);
  }

  dispose() { this.el.remove(); }
}
