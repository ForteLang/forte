// .frec encoding in the browser (mirror of crates/fortelang/src/frec.rs):
// FREC1 magic, u32-le header length, JSON header with the mandatory
// provenance block, f32-le PCM.

export function encodeFrec(rate, channels, pcm, provenance) {
  const header = new TextEncoder().encode(
    JSON.stringify({ rate, ch: channels, provenance })
  );
  const out = new Uint8Array(6 + 4 + header.length + pcm.length * 4);
  out.set([0x46, 0x52, 0x45, 0x43, 0x31, 0x0a]); // "FREC1\n"
  new DataView(out.buffer).setUint32(6, header.length, true);
  out.set(header, 10);
  new Uint8Array(out.buffer, 10 + header.length).set(new Uint8Array(pcm.buffer, pcm.byteOffset, pcm.length * 4));
  return out;
}

export function toBase64(bytes) {
  let s = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    s += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(s);
}

export function fromBase64(b64) {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
