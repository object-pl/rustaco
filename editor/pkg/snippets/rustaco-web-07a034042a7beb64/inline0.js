
export function draw_blocks(ctx, ops, colors) {
  const c = colors.split('\n');
  let color = -1;
  for (let i = 0; i < ops.length; i += 5) {
    if (ops[i + 4] !== color) { color = ops[i + 4]; ctx.fillStyle = c[color]; }
    ctx.fillRect(ops[i], ops[i + 1], ops[i + 2], ops[i + 3]);
  }
}
export function draw_runs(ctx, text, ops, colors, fonts) {
  const c = colors.split('\n'), f = fonts.split('\n');
  let font = -1, color = -1;
  for (let i = 0; i < ops.length; i += 7) {
    if (ops[i + 5] !== font) { font = ops[i + 5]; ctx.font = f[font]; }
    if (ops[i + 4] !== color) { color = ops[i + 4]; ctx.fillStyle = c[color]; }
    const run = text.substring(ops[i + 2], ops[i + 3]);
    ctx.fillText(run, ops[i], ops[i + 1]);
    if (ops[i + 6] > 0) {
      ctx.fillRect(ops[i], ops[i + 1] + ops[i + 6], ctx.measureText(run).width, 1);
    }
  }
}