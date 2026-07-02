// Local-first project storage on OPFS (Origin Private File System): songs
// survive reloads and offline use without any server. Chromium-first — Safari
// needs the worker/SyncAccessHandle path and 7-day-eviction mitigation later
// (SAD degradation matrix).

export class Store {
  async init() {
    const root = await navigator.storage.getDirectory();
    this.dir = await root.getDirectoryHandle('songs', { create: true });
    // ask the browser not to evict our songs under storage pressure
    try { await navigator.storage.persist(); } catch { /* best effort */ }
    return this;
  }

  async list() {
    const out = [];
    for await (const [name, handle] of this.dir.entries()) {
      if (handle.kind === 'file' && name.endsWith('.forte')) out.push(name);
    }
    return out.sort();
  }

  async read(name) {
    const handle = await this.dir.getFileHandle(name);
    return (await handle.getFile()).text();
  }

  async write(name, text) {
    const handle = await this.dir.getFileHandle(name, { create: true });
    const w = await handle.createWritable();
    await w.write(text);
    await w.close();
  }

  async remove(name) {
    await this.dir.removeEntry(name);
  }
}
