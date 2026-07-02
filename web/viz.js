// Read-only arrangement view: tracks as lanes, clips as blocks with note
// previews, and a playhead. Pure projection of the compiled project — no
// editing (the code is the only source of truth).

export class Viz {
  constructor(canvas) {
    this.canvas = canvas;
    this.g = canvas.getContext('2d');
    this.data = null;
    this.beats = 0; // playhead position
    const redraw = () => this.draw();
    new ResizeObserver(redraw).observe(canvas);
  }

  setData(data) {
    this.data = data;
    this.draw();
  }

  setPlayhead(beats) {
    this.beats = beats;
    this.draw();
  }

  draw() {
    const { canvas, g, data } = this;
    const dpr = devicePixelRatio || 1;
    const w = canvas.clientWidth, h = canvas.clientHeight;
    if (canvas.width !== w * dpr) { canvas.width = w * dpr; canvas.height = h * dpr; }
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    if (!data || !data.tracks?.length) return;

    const headerW = 92;
    const rulerH = 16;
    const laneH = (h - rulerH) / data.tracks.length;
    const span = Math.max(data.lengthBeats, data.beatsPerBar);
    const bx = (beats) => headerW + ((w - headerW) * beats) / span;

    // bar ruler
    g.font = '9px ui-monospace, monospace';
    g.textBaseline = 'top';
    for (let b = 0; b * data.beatsPerBar <= span; b++) {
      const x = bx(b * data.beatsPerBar);
      g.strokeStyle = b % 4 === 0 ? '#2f3440' : '#232730';
      g.beginPath(); g.moveTo(x, rulerH); g.lineTo(x, h); g.stroke();
      if (b % 4 === 0) { g.fillStyle = '#565d69'; g.fillText(String(b + 1), x + 3, 3); }
    }

    data.tracks.forEach((t, i) => {
      const y = rulerH + i * laneH;
      const [r, gg, b] = t.color;
      g.strokeStyle = '#20242c';
      g.beginPath(); g.moveTo(0, y + laneH); g.lineTo(w, y + laneH); g.stroke();
      g.fillStyle = '#8a919e';
      g.font = '10px ui-sans-serif, system-ui';
      g.textBaseline = 'middle';
      g.fillText(t.name + (t.fx ? ' ⟲' : ''), 8, y + laneH / 2, headerW - 14);

      for (const c of t.clips) {
        const x0 = bx(c.start), x1 = bx(c.start + c.duration);
        g.fillStyle = `rgba(${r},${gg},${b},0.22)`;
        g.strokeStyle = `rgb(${r},${gg},${b})`;
        g.fillRect(x0, y + 2, x1 - x0, laneH - 5);
        g.strokeRect(x0 + 0.5, y + 2.5, x1 - x0 - 1, laneH - 6);
        // note preview: content loops inside the placed duration
        const pitches = c.notes.map((n) => n[0]);
        if (!pitches.length) continue;
        const lo = Math.min(...pitches), hi = Math.max(...pitches);
        const py = (p) =>
          y + laneH - 6 - (hi === lo ? 0.5 : (p - lo) / (hi - lo)) * (laneH - 12);
        g.fillStyle = `rgb(${r},${gg},${b})`;
        for (let off = 0; off < c.duration; off += c.length) {
          for (const [p, s, len] of c.notes) {
            if (off + s >= c.duration) continue;
            const nx = bx(c.start + off + s);
            const nw = Math.max(1.5, bx(Math.min(c.duration, off + s + len)) - bx(off + s));
            g.fillRect(nx, py(p), nw, 2);
          }
        }
      }
    });

    // playhead
    const x = bx(Math.min(this.beats, span));
    g.strokeStyle = '#e8b34c';
    g.lineWidth = 1.5;
    g.beginPath(); g.moveTo(x, 0); g.lineTo(x, h); g.stroke();
    g.lineWidth = 1;
  }
}
