// Local-first project storage on OPFS (Origin Private File System): songs
// survive reloads and offline use without any server. Supports nested paths
// ("devices/warm.forte") so imported libraries keep their import paths.
// Chromium-first — Safari needs the worker/SyncAccessHandle path and
// 7-day-eviction mitigation later (SAD degradation matrix).

export class Store {
  async init() {
    const root = await navigator.storage.getDirectory();
    this.dir = await root.getDirectoryHandle('songs', { create: true });
    // ask the browser not to evict our songs under storage pressure
    try { await navigator.storage.persist(); } catch { /* best effort */ }
    return this;
  }

  async resolve(path, create) {
    const parts = path.split('/').filter(Boolean);
    const base = parts.pop();
    let dir = this.dir;
    for (const p of parts) {
      dir = await dir.getDirectoryHandle(p, { create });
    }
    return { dir, base };
  }

  async list(ext = '.forte') {
    const out = [];
    const walk = async (dir, prefix) => {
      for await (const [name, handle] of dir.entries()) {
        if (handle.kind === 'file' && name.endsWith(ext)) {
          out.push(prefix + name);
        } else if (handle.kind === 'directory') {
          await walk(handle, `${prefix}${name}/`);
        }
      }
    };
    await walk(this.dir, '');
    return out.sort();
  }

  async readBytes(path) {
    const { dir, base } = await this.resolve(path, false);
    const handle = await dir.getFileHandle(base);
    return new Uint8Array(await (await handle.getFile()).arrayBuffer());
  }

  async writeBytes(path, bytes) {
    const { dir, base } = await this.resolve(path, true);
    const handle = await dir.getFileHandle(base, { create: true });
    const w = await handle.createWritable();
    await w.write(bytes);
    await w.close();
  }

  async read(path) {
    const { dir, base } = await this.resolve(path, false);
    const handle = await dir.getFileHandle(base);
    return (await handle.getFile()).text();
  }

  async write(path, text) {
    const { dir, base } = await this.resolve(path, true);
    const handle = await dir.getFileHandle(base, { create: true });
    const w = await handle.createWritable();
    await w.write(text);
    await w.close();
  }

  async remove(path) {
    const { dir, base } = await this.resolve(path, false);
    await dir.removeEntry(base);
  }
}

// Project mode (forte daw): the store IS the real package directory on
// disk, served by the local project API. Same interface as Store, so the
// whole editor (tree, autosave, modules, history snapshots) follows the
// project without knowing which backend it is on.
export class ServerStore {
  async init() {
    const r = await fetch('api/project');
    if (!r.ok) throw new Error('no project api');
    this.project = await r.json();
    return this;
  }

  async refresh() {
    try { this.project = await (await fetch('api/project')).json(); } catch { /* keep last */ }
  }

  async list(ext = '.forte') {
    const r = await fetch(`api/list?ext=${encodeURIComponent(ext)}`);
    return r.ok ? r.json() : [];
  }

  async read(path) {
    const r = await fetch(`api/src?path=${encodeURIComponent(path)}`);
    if (!r.ok) throw new Error(`read ${path}: ${r.status}`);
    return r.text();
  }

  async write(path, text) {
    await fetch(`api/src?path=${encodeURIComponent(path)}`, { method: 'POST', body: text });
  }

  async readBytes(path) {
    const r = await fetch(`api/src?path=${encodeURIComponent(path)}`);
    if (!r.ok) throw new Error(`read ${path}: ${r.status}`);
    return new Uint8Array(await r.arrayBuffer());
  }

  async writeBytes(path, bytes) {
    await fetch(`api/src?path=${encodeURIComponent(path)}`, { method: 'POST', body: bytes });
  }

  // project files are deleted in the shell / git, not from the DAW (v1)
  async remove() {}

  // one round-trip for the whole import map (instead of a request per file)
  async readAllText() {
    const r = await fetch('api/modules');
    return r.ok ? r.json() : {};
  }

  async readAllAssets() {
    const r = await fetch('api/assets');
    return r.ok ? r.json() : {};
  }
}
