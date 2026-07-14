// Read-only visualization: the arrangement (tracks as lanes, clips as blocks
// with note previews), a per-track PIANO ROLL, live level METERS, and a
// playhead. Pure projection of the compiled project — no editing (the code
// is the only source of truth). Every element knows its source line, so
// hosts can wire clicks to code-jumps via hitTest().

export class Viz {
  constructor(canvas) {
    this.canvas = canvas;
    this.g = canvas.getContext('2d');
    this.data = null;
    this.beats = 0; // playhead position
    this.mode = 'arrange'; // 'arrange' | 'piano'
    this.rollTrack = 0; // piano roll subject
    this.peaks = null; // per-track peaks 0..1 (same order as data.tracks)
    const redraw = () => this.draw();
    new ResizeObserver(redraw).observe(canvas);
  }

  setData(data) {
    this.data = data;
    if (data?.tracks && this.rollTrack >= data.tracks.length) this.rollTrack = 0;
    this.draw();
  }

  setPlayhead(beats) {
    this.beats = beats;
    this.draw();
  }

  // Live per-track levels (same order as data.tracks). Call at pos-message rate.
  setPeaks(peaks) {
    this.peaks = peaks;
    this.draw();
  }

  // Toggle the piano roll for a track (same track toggles back to arrange).
  togglePianoRoll(track) {
    if (this.mode === 'piano' && this.rollTrack === track) {
      this.mode = 'arrange';
    } else {
      this.mode = 'piano';
      this.rollTrack = track;
    }
    this.draw();
  }

  // Horizontal geometry of the arrange view (for drag math in the host).
  geom() {
    const { data } = this;
    const w = this.canvas.clientWidth;
    const span = Math.max(data?.lengthBeats ?? 0, data?.beatsPerBar ?? 4);
    return { headerW: 92, w, span, pxPerBeat: (w - 92) / span };
  }

  // What is under (x, y) in CSS pixels?
  //   {kind: 'roll'}                — piano roll is showing (click = back)
  //   {kind: 'header', track}       — lane header (piano-roll toggle)
  //   {kind: 'clip', track, line, start, duration} — a clip (jump / drag)
  //   {kind: 'lane', track, line}   — empty lane space (the track's line)
  hitTest(x, y) {
    const { data } = this;
    if (!data?.tracks?.length) return null;
    if (this.mode === 'piano') return { kind: 'roll', track: this.rollTrack };
    const h = this.canvas.clientHeight;
    const rulerH = 16;
    const laneH = (h - rulerH) / data.tracks.length;
    const i = Math.floor((y - rulerH) / laneH);
    const t = data.tracks[i];
    if (!t) return null;
    if (x < 92) return { kind: 'header', track: i };
    const { span } = this.geom();
    const w = this.canvas.clientWidth;
    const beats = ((x - 92) / (w - 92)) * span;
    for (const c of t.clips) {
      if (beats >= c.start && beats <= c.start + c.duration) {
        return {
          kind: 'clip', track: i, line: c.line || t.line || 0,
          start: c.start, duration: c.duration,
        };
      }
    }
    return { kind: 'lane', track: i, line: t.line || 0 };
  }

  // A drag ghost: the clip's outline shown at its candidate position while
  // the pointer is down (the write happens on drop, through the edit layer).
  setGhost(ghost) {
    this.ghost = ghost; // {track, start, duration} | null
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
    if (this.mode === 'piano') this.drawPianoRoll(w, h);
    else this.drawArrange(w, h);
  }

  drawArrange(w, h) {
    const { g, data } = this;
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
      g.fillText(t.name + (t.fx ? ' ⟲' : ''), 8, y + laneH / 2, headerW - 24);

      // live meter: a thin bar riding the header's right edge
      const peak = this.peaks?.[i] ?? 0;
      if (peak > 0.003) {
        const mh = Math.min(1, peak) * (laneH - 6);
        g.fillStyle = peak > 0.9 ? '#e06c75' : `rgb(${r},${gg},${b})`;
        g.fillRect(headerW - 9, y + laneH - 3 - mh, 5, mh);
      }

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

    // drag ghost: dashed outline at the candidate bar position
    if (this.ghost) {
      const { track, start, duration } = this.ghost;
      const y = rulerH + track * laneH;
      g.save();
      g.setLineDash([4, 3]);
      g.strokeStyle = '#e8b34c';
      g.strokeRect(bx(start) + 0.5, y + 2.5, bx(start + duration) - bx(start) - 1, laneH - 6);
      g.restore();
    }

    this.drawPlayhead(bx, h, span);
  }

  // Piano roll of one track: pitch rows over the whole arrangement, note
  // velocity as opacity. Clip loops are unrolled — you see what plays.
  drawPianoRoll(w, h) {
    const { g, data } = this;
    const t = data.tracks[this.rollTrack];
    if (!t) return;
    const headerW = 34;
    const rulerH = 16;
    const span = Math.max(data.lengthBeats, data.beatsPerBar);
    const bx = (beats) => headerW + ((w - headerW) * beats) / span;

    // collect sounding notes (loops unrolled)
    const notes = [];
    let lo = 127, hi = 0;
    for (const c of t.clips) {
      for (let off = 0; off < c.duration; off += c.length) {
        for (const [p, s, len, vel] of c.notes) {
          if (off + s >= c.duration) continue;
          notes.push([p, c.start + off + s, Math.min(len, c.duration - off - s), vel ?? 0.8]);
          if (p < lo) lo = p;
          if (p > hi) hi = p;
        }
      }
    }
    g.fillStyle = '#8a919e';
    g.font = '10px ui-sans-serif, system-ui';
    g.textBaseline = 'top';
    g.fillText(`♪ ${t.name} — piano roll (click to go back)`, headerW + 6, 2);
    if (!notes.length) return;
    lo = Math.max(0, lo - 2);
    hi = Math.min(127, hi + 2);
    const rows = hi - lo + 1;
    const rowH = (h - rulerH) / rows;
    const py = (p) => rulerH + (hi - p) * rowH;

    // row shading (black keys darker) + octave labels
    for (let p = lo; p <= hi; p++) {
      const black = [1, 3, 6, 8, 10].includes(p % 12);
      g.fillStyle = black ? 'rgba(0,0,0,0.22)' : 'rgba(255,255,255,0.02)';
      g.fillRect(headerW, py(p), w - headerW, rowH);
      if (p % 12 === 0 && rowH >= 5) {
        g.fillStyle = '#565d69';
        g.font = '8px ui-monospace, monospace';
        g.textBaseline = 'middle';
        g.fillText(`C${Math.floor(p / 12) - 1}`, 4, py(p) + rowH / 2);
      }
    }
    // bar lines
    for (let bnum = 0; bnum * data.beatsPerBar <= span; bnum++) {
      const x = bx(bnum * data.beatsPerBar);
      g.strokeStyle = bnum % 4 === 0 ? '#2f3440' : '#232730';
      g.beginPath(); g.moveTo(x, rulerH); g.lineTo(x, h); g.stroke();
    }
    // the notes, velocity as opacity
    const [r, gg, b] = t.color;
    for (const [p, s, len, vel] of notes) {
      const x0 = bx(s);
      const nw = Math.max(2, bx(s + len) - x0);
      g.fillStyle = `rgba(${r},${gg},${b},${0.35 + 0.65 * vel})`;
      g.fillRect(x0, py(p) + 0.5, nw, Math.max(1.5, rowH - 1));
    }

    this.drawPlayhead(bx, h, span);
  }

  drawPlayhead(bx, h, span) {
    const { g } = this;
    const x = bx(Math.min(this.beats, span));
    g.strokeStyle = '#e8b34c';
    g.lineWidth = 1.5;
    g.beginPath(); g.moveTo(x, 0); g.lineTo(x, h); g.stroke();
    g.lineWidth = 1;
  }
}
