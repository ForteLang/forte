// Forte AudioWorkletProcessor: owns its own wasm instance of the compiler +
// engine. The main thread sends raw source text; we compile *on the audio
// thread* (fine at prototype scale) and swap the running project without
// stopping the transport — hot reload in the browser.

class ForteProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.ready = false;
    this.blocks = 0;
    this.port.onmessage = (e) => this.onMessage(e.data);
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
        if (!this.ready) return;
        const stage = (json, commit) => {
          const mb = new TextEncoder().encode(json);
          const mp = this.e.fw_modules_prepare(this.ctx, mb.length);
          new Uint8Array(this.e.memory.buffer, mp, mb.length).set(mb);
          commit(this.ctx);
        };
        if (msg.modules) stage(msg.modules, this.e.fw_modules_commit);
        if (msg.assets) stage(msg.assets, this.e.fw_assets_commit);
        const bytes = new TextEncoder().encode(msg.text);
        const ptr = this.e.fw_src_prepare(this.ctx, bytes.length);
        new Uint8Array(this.e.memory.buffer, ptr, bytes.length).set(bytes);
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
      this.port.postMessage({
        kind: 'pos',
        beats: this.e.fw_position(this.ctx),
        peak: this.e.fw_master_peak(this.ctx),
      });
    }
    return true;
  }
}

registerProcessor('forte', ForteProcessor);
