// Microphone capture processor: raw PCM straight off the audio thread
// (SRS-REC-002 — no MediaRecorder, no opaque compression). Chunks are
// transferred to the main thread which assembles the .frec take.

class ForteRecorder extends AudioWorkletProcessor {
  process(inputs) {
    const ch = inputs[0]?.[0];
    if (ch && ch.length) {
      const copy = new Float32Array(ch);
      this.port.postMessage(copy, [copy.buffer]);
    }
    return true;
  }
}

registerProcessor('forte-rec', ForteRecorder);
