// The WebGL renderer. Rust paints each frame as quads in WASM memory (rectangles and glyphs,
// back to front) and lists the glyphs it has not seen before; this file draws new glyphs
// into the atlas, uploads the quads and issues one draw call per frame.

const VERTEX = `#version 300 es
layout(location = 0) in vec4 a_rect;
layout(location = 1) in vec2 a_origin;
layout(location = 2) in uint a_slot;
layout(location = 3) in vec4 a_color;
uniform vec2 u_view;
uniform vec2 u_slot;
uniform float u_cols;
uniform float u_atlas;
uniform sampler2D u_flags;
out vec2 v_uv;
out vec4 v_color;
flat out float v_colored;
void main() {
  vec2 corner = vec2(float(gl_VertexID & 1), float(gl_VertexID >> 1));
  vec2 pos = a_rect.xy + corner * a_rect.zw;
  gl_Position = vec4(pos.x / u_view.x * 2.0 - 1.0, 1.0 - pos.y / u_view.y * 2.0, 0.0, 1.0);
  float s = float(a_slot);
  vec2 cell = vec2(mod(s, u_cols), floor(s / u_cols));
  // Slot 0 is solid: rectangles sample its middle. A glyph quad may be cut by a clip, so
  // its texture position follows the pixel's offset from the slot's origin.
  v_uv = a_slot == 0u ? (vec2(0.5) * u_slot) / u_atlas : (cell * u_slot + pos - a_origin) / u_atlas;
  v_colored = texelFetch(u_flags, ivec2(cell), 0).r;
  v_color = a_color;
}`;

const FRAGMENT = `#version 300 es
precision highp float;
uniform sampler2D u_tex;
in vec2 v_uv;
in vec4 v_color;
flat in float v_colored;
out vec4 color;
void main() {
  vec4 t = texture(u_tex, v_uv);
  // Colour glyphs (emoji) keep their own colours; the rest take the run's colour.
  color = v_colored > 0.5 ? vec4(t.rgb * t.a, t.a) : vec4(v_color.rgb, 1.0) * (t.a * v_color.a);
}`;

const VARIANTS = ['', 'italic ', 'bold ', 'italic bold '];
const STRIDE = 32;

export class WebGLText {
  /**
   * Throws when WebGL2 is not available. `onLost` is called when the browser drops the
   * context (GPU reset, too many contexts); the editor then goes back to the canvas.
   */
  constructor(memory, fontFamily, fontSize, onLost) {
    this.memory = memory;
    this.fontFamily = fontFamily;
    this.fontSize = fontSize;
    this.el = document.createElement('canvas');
    this.el.style.cssText = 'position:absolute;inset:0;width:100%;height:100%;pointer-events:none';
    const gl = this.el.getContext('webgl2', { alpha: true, premultipliedAlpha: true, antialias: false, depth: false });
    if (!gl) throw new Error('WebGL2 is not available');
    this.gl = gl;
    this.el.addEventListener('webglcontextlost', (e) => {
      e.preventDefault();
      if (this.lost) return;
      this.lost = true;
      onLost();
    });
    this.#setup();
    this.raster = document.createElement('canvas');
    this.rasterCtx = this.raster.getContext('2d', { willReadFrequently: true });
  }

  #setup() {
    const { gl } = this;
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
    this.u = Object.fromEntries(
      ['u_view', 'u_slot', 'u_cols', 'u_atlas', 'u_tex', 'u_flags'].map((n) => [n, gl.getUniformLocation(p, n)]),
    );
    this.vao = gl.createVertexArray();
    gl.bindVertexArray(this.vao);
    this.buffer = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 4, gl.FLOAT, false, STRIDE, 0);
    gl.vertexAttribDivisor(0, 1);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 2, gl.FLOAT, false, STRIDE, 16);
    gl.vertexAttribDivisor(1, 1);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribIPointer(2, 1, gl.UNSIGNED_INT, STRIDE, 24);
    gl.vertexAttribDivisor(2, 1);
    gl.enableVertexAttribArray(3);
    gl.vertexAttribPointer(3, 4, gl.UNSIGNED_BYTE, true, STRIDE, 28);
    gl.vertexAttribDivisor(3, 1);
    const texture = () => {
      const t = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, t);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      return t;
    };
    this.texture = texture();
    this.flagsTexture = texture();
    this.flags = null;
  }

  /** Clears the atlas and its colour flags (one byte per slot), and fills the solid slot. */
  #resetAtlas([w, h, , cols, side]) {
    const { gl } = this;
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, side, side, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    const solid = new Uint8Array(w * h * 4).fill(255);
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, solid);
    const rows = Math.floor(side / h);
    this.flags = new Uint8Array(cols * rows);
    this.flagsSize = [cols, rows];
    gl.bindTexture(gl.TEXTURE_2D, this.flagsTexture);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, cols, rows, 0, gl.RED, gl.UNSIGNED_BYTE, this.flags);
  }

  /** Draws the glyphs Rust has not seen before into their atlas slots. */
  #rasterize(core, geometry, dpr) {
    const { gl } = this;
    const [w, h, pad, cols, side, reset] = geometry;
    if (reset || !this.flags) this.#resetAtlas(geometry);
    const chars = [...core.glPendingChars()];
    const pending = core.glTakePending();
    if (!pending.length) return;
    // New slots are consecutive: draw the rows of slots they fill on a scratch canvas and
    // upload that band.
    const first = Math.floor(pending[0] / cols);
    const last = Math.floor(pending[pending.length - 3] / cols);
    const bandH = (last - first + 1) * h;
    const r = this.raster;
    if (r.width !== side) r.width = side;
    if (r.height < bandH) r.height = bandH;
    const ctx = this.rasterCtx;
    ctx.clearRect(0, 0, side, bandH);
    ctx.textBaseline = 'middle';
    let variant = -1;
    for (let i = 0, g = 0; i < pending.length; i += 3, g++) {
      const slot = pending[i];
      if (pending[i + 1] !== variant) {
        variant = pending[i + 1];
        ctx.font = `${VARIANTS[variant & 3]}${this.fontSize * dpr}px ${this.fontFamily}`;
        // Dark text is drawn dark: browsers shape glyph coverage by the text's brightness.
        ctx.fillStyle = variant & 4 ? '#000' : '#fff';
      }
      ctx.fillText(chars[g], (slot % cols) * w + pad + pending[i + 2] / 4, (Math.floor(slot / cols) - first) * h + h / 2);
    }
    const data = ctx.getImageData(0, 0, side, bandH);
    // A glyph drawn white that came out in colour is an emoji (or another colour font glyph).
    for (let i = 0, g = 0; i < pending.length; i += 3, g++) {
      if (chars[g].charCodeAt(0) < 0x80) continue;
      const slot = pending[i];
      const x0 = (slot % cols) * w;
      const y0 = (Math.floor(slot / cols) - first) * h;
      let colored = false;
      for (let y = y0; y < y0 + h && !colored; y++) {
        for (let x = x0, o = (y * side + x0) * 4; x < x0 + w; x++, o += 4) {
          if (data.data[o + 3] > 64 && (data.data[o] !== data.data[o + 1] || data.data[o + 1] !== data.data[o + 2])) {
            colored = true;
            break;
          }
        }
      }
      if (colored) this.flags[slot] = 255;
    }
    // Upload only the new slots, a run of neighbours in one atlas row at a time: the rest of
    // a row may hold glyphs from earlier frames, which the scratch band does not have.
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    const pixels = new Uint8Array(data.data.buffer);
    gl.pixelStorei(gl.UNPACK_ROW_LENGTH, side);
    for (let i = 0; i < pending.length;) {
      const start = pending[i];
      let end = start + 1;
      i += 3;
      while (i < pending.length && pending[i] === end && Math.floor(end / cols) === Math.floor(start / cols)) {
        end++;
        i += 3;
      }
      const row = Math.floor(start / cols);
      const x0 = (start % cols) * w;
      gl.pixelStorei(gl.UNPACK_SKIP_PIXELS, x0);
      gl.pixelStorei(gl.UNPACK_SKIP_ROWS, (row - first) * h);
      gl.texSubImage2D(gl.TEXTURE_2D, 0, x0, row * h, (end - start) * w, h, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    }
    gl.pixelStorei(gl.UNPACK_ROW_LENGTH, 0);
    gl.pixelStorei(gl.UNPACK_SKIP_PIXELS, 0);
    gl.pixelStorei(gl.UNPACK_SKIP_ROWS, 0);
    gl.bindTexture(gl.TEXTURE_2D, this.flagsTexture);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, this.flagsSize[0], this.flagsSize[1], gl.RED, gl.UNSIGNED_BYTE, this.flags);
  }

  /** Draws the last frame Rust laid out. */
  draw(core, dpr) {
    if (this.lost) return;
    const { gl, el } = this;
    const geometry = core.glGeometry();
    if (!geometry.length) return;
    const width = Math.round(el.clientWidth * dpr);
    const height = Math.round(el.clientHeight * dpr);
    if (el.width !== width || el.height !== height) {
      el.width = width;
      el.height = height;
    }
    this.#rasterize(core, geometry, dpr);
    const [ptr, count] = core.glInstances();
    gl.viewport(0, 0, width, height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (!count) return;
    gl.useProgram(this.program);
    gl.bindVertexArray(this.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.buffer);
    gl.bufferData(gl.ARRAY_BUFFER, new Uint8Array(this.memory.buffer, ptr, count * STRIDE), gl.STREAM_DRAW);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.uniform1i(this.u.u_tex, 0);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, this.flagsTexture);
    gl.uniform1i(this.u.u_flags, 1);
    gl.uniform2f(this.u.u_view, width, height);
    gl.uniform2f(this.u.u_slot, geometry[0], geometry[1]);
    gl.uniform1f(this.u.u_cols, geometry[3]);
    gl.uniform1f(this.u.u_atlas, geometry[4]);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, count);
  }

  /**
   * The last frame as a 2D canvas. Reads the pixels back right after drawing (call it in the
   * same task as `draw`): some browsers hand `drawImage` a stale buffer of a WebGL canvas.
   */
  toCanvas() {
    const { gl, el } = this;
    const { width, height } = el;
    const px = new Uint8ClampedArray(width * height * 4);
    if (!this.lost) gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, px);
    // Bottom-up and premultiplied to top-down and straight alpha.
    const out = new ImageData(width, height);
    for (let y = 0; y < height; y++) {
      const src = (height - 1 - y) * width * 4;
      const dst = y * width * 4;
      for (let i = 0; i < width * 4; i += 4) {
        const a = px[src + i + 3];
        if (!a) continue;
        out.data[dst + i] = (px[src + i] * 255) / a;
        out.data[dst + i + 1] = (px[src + i + 1] * 255) / a;
        out.data[dst + i + 2] = (px[src + i + 2] * 255) / a;
        out.data[dst + i + 3] = a;
      }
    }
    const c = document.createElement('canvas');
    c.width = width;
    c.height = height;
    c.getContext('2d').putImageData(out, 0, 0);
    return c;
  }

  dispose() {
    this.lost = true;
    this.gl.getExtension('WEBGL_lose_context')?.loseContext();
    this.el.remove();
  }
}
