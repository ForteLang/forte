// Forte AudioWorkletProcessor: owns its own wasm instance of the compiler +
// engine. The main thread sends raw source text; we compile *on the audio
// thread* (fine at prototype scale) and swap the running project without
// stopping the transport — hot reload in the browser.

class ForteProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.ready = false;
    this.blocks = 0;
    this.port.onmessage = (e) =>
      Promise.resolve(this.onMessage(e.data)).catch((err) =>
        this.port.postMessage({ kind: 'err', message: String((err && err.stack) || err) })
      );
  }

  async onMessage(msg) {
    switch (msg.cmd) {
      case 'init': {
        const { instance } = await WebAssembly.instantiate(msg.wasm, {});
        this.e = instance.exports;
        this.ctx = this.e.fw_new(sampleRate);
        this.ready = true;
        this.port.postMessage({ kind: 'ready' });
        break;
      }
      case 'src': {
        // NOTE: AudioWorkletGlobalScope has no TextEncoder — the main thread
        // sends pre-encoded Uint8Arrays. (A TextEncoder here once silenced
        // the whole editor: the throw vanished into the async handler.)
        if (!this.ready) return;
        const stage = (bytes, commit) => {
          const mp = this.e.fw_modules_prepare(this.ctx, bytes.length);
          new Uint8Array(this.e.memory.buffer, mp, bytes.length).set(bytes);
          commit(this.ctx);
        };
        if (msg.modules) stage(msg.modules, this.e.fw_modules_commit);
        if (msg.assets) stage(msg.assets, this.e.fw_assets_commit);
        const ptr = this.e.fw_src_prepare(this.ctx, msg.text.length);
        new Uint8Array(this.e.memory.buffer, ptr, msg.text.length).set(msg.text);
        const n = this.e.fw_compile(this.ctx);
        this.port.postMessage({ kind: 'compiled', diagCount: n });
        break;
      }
      case 'play':
        if (this.ready) this.e.fw_play(this.ctx);
        break;
      case 'note':
        if (this.ready) this.e.fw_note(this.ctx, msg.on ? 1 : 0, msg.pitch, msg.vel ?? 0.8);
        break;
      case 'stop':
        if (this.ready) this.e.fw_stop(this.ctx);
        break;
      case 'seek':
        if (this.ready) this.e.fw_seek(this.ctx, msg.beats);
        break;
      // open-stems: listener-side stem controls
      case 'mute':
        if (this.ready) this.e.fw_set_mute(this.ctx, msg.track, msg.on ? 1 : 0);
        break;
      case 'solo':
        if (this.ready) this.e.fw_set_solo(this.ctx, msg.track, msg.on ? 1 : 0);
        break;
    }
  }

  process(_inputs, outputs) {
    if (!this.ready) return true;
    const out = outputs[0];
    const frames = out[0].length; // 128
    this.e.fw_process(this.ctx, frames);
    out[0].set(new Float32Array(this.e.memory.buffer, this.e.fw_out_l(this.ctx), frames));
    (out[1] ?? out[0]).set(
      new Float32Array(this.e.memory.buffer, this.e.fw_out_r(this.ctx), frames)
    );
    if (++this.blocks % 16 === 0) {
      const n = this.e.fw_debug_tracks(this.ctx);
      const peaks = new Array(n);
      for (let i = 0; i < n; i++) peaks[i] = this.e.fw_track_peak(this.ctx, i);
      this.port.postMessage({
        kind: 'pos',
        beats: this.e.fw_position(this.ctx),
        peak: this.e.fw_master_peak(this.ctx),
        peaks,
      });
    }
    return true;
  }
}

registerProcessor('forte', ForteProcessor);
