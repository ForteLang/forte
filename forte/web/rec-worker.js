// Recording durability worker (SRS-REC-002): PCM chunks are written straight
// to OPFS through a SyncAccessHandle and flushed every second, so a tab crash
// mid-take loses at most the last second — the boot path recovers the rest.
// (createWritable would not do: it only commits on close, i.e. exactly never
// in a crash.)

let pcm = null;
let offset = 0;
let lastFlush = 0;

async function assetsDir() {
  const root = await navigator.storage.getDirectory();
  const songs = await root.getDirectoryHandle('songs', { create: true });
  return songs.getDirectoryHandle('assets', { create: true });
}

onmessage = async (e) => {
  const m = e.data;
  if (m.cmd === 'start') {
    const dir = await assetsDir();
    // journal first: rate/session/time survive with the samples
    const journalFile = await dir.getFileHandle('.recording.json', { create: true });
    const jh = await journalFile.createSyncAccessHandle();
    jh.truncate(0);
    jh.write(
      new TextEncoder().encode(
        JSON.stringify({ rate: m.rate, started_at: m.startedAt, session: m.session })
      ),
      { at: 0 }
    );
    jh.flush();
    jh.close();

    const pcmFile = await dir.getFileHandle('.recording.pcm', { create: true });
    pcm = await pcmFile.createSyncAccessHandle();
    pcm.truncate(0);
    offset = 0;
    lastFlush = 0;
    postMessage({ kind: 'started' });
  } else if (m.cmd === 'chunk') {
    if (!pcm) return;
    const bytes = new Uint8Array(m.data.buffer, m.data.byteOffset, m.data.byteLength);
    pcm.write(bytes, { at: offset });
    offset += bytes.byteLength;
    if (offset - lastFlush >= 48_000 * 4) {
      pcm.flush(); // durable once per second of audio
      lastFlush = offset;
    }
  } else if (m.cmd === 'stop') {
    if (pcm) {
      pcm.flush();
      pcm.close();
      pcm = null;
    }
    postMessage({ kind: 'stopped', bytes: offset });
  }
};
