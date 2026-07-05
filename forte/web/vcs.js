// The .forte repository, in the browser: commit / log / restore over OPFS.
// Same object format as the CLI (SHA-256 content addressing, "kind len\0"
// framing, JSON trees/commits) so browser repos and CLI repos speak the same
// language. The semantic diff itself runs in the wasm compiler (fw_semdiff).

const te = new TextEncoder();
const td = new TextDecoder();

async function sha256hex(bytes) {
  const h = await crypto.subtle.digest('SHA-256', bytes);
  return [...new Uint8Array(h)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

export class Vcs {
  async init() {
    const root = await navigator.storage.getDirectory();
    this.dir = await root.getDirectoryHandle('vcs', { create: true });
    return this;
  }

  async _resolve(path, create) {
    const parts = path.split('/').filter(Boolean);
    const base = parts.pop();
    let dir = this.dir;
    for (const p of parts) dir = await dir.getDirectoryHandle(p, { create });
    return { dir, base };
  }

  async _read(path) {
    try {
      const { dir, base } = await this._resolve(path, false);
      const f = await (await dir.getFileHandle(base)).getFile();
      return new Uint8Array(await f.arrayBuffer());
    } catch {
      return null;
    }
  }

  async _write(path, bytes) {
    const { dir, base } = await this._resolve(path, true);
    const w = await (await dir.getFileHandle(base, { create: true })).createWritable();
    await w.write(bytes);
    await w.close();
  }

  // ---- object store ---------------------------------------------------------

  async putObj(kind, body) {
    const header = te.encode(`${kind} ${body.length}\u0000`);
    const data = new Uint8Array(header.length + body.length);
    data.set(header);
    data.set(body, header.length);
    const hash = await sha256hex(data);
    const path = `objects/${hash.slice(0, 2)}/${hash.slice(2)}`;
    if (!(await this._read(path))) await this._write(path, data);
    return hash;
  }

  async getObj(hash) {
    const data = await this._read(`objects/${hash.slice(0, 2)}/${hash.slice(2)}`);
    if (!data) throw new Error(`オブジェクト ${hash.slice(0, 8)} がありません`);
    const nul = data.indexOf(0);
    return { kind: td.decode(data.slice(0, nul)).split(' ')[0], body: data.slice(nul + 1) };
  }

  // ---- trees ({path: text} ↔ nested tree objects) ---------------------------

  async writeTree(snap) {
    const rootNode = { files: {}, dirs: {} };
    for (const [path, text] of Object.entries(snap)) {
      const parts = path.split('/');
      let node = rootNode;
      for (const p of parts.slice(0, -1)) {
        node = node.dirs[p] ??= { files: {}, dirs: {} };
      }
      node.files[parts[parts.length - 1]] = text;
    }
    const store = async (node) => {
      const entries = [];
      for (const name of Object.keys(node.files).sort()) {
        const hash = await this.putObj('blob', te.encode(node.files[name]));
        entries.push({ hash, kind: 'blob', name }); // alphabetical keys = serde_json order
      }
      for (const name of Object.keys(node.dirs).sort()) {
        entries.push({ hash: await store(node.dirs[name]), kind: 'tree', name });
      }
      return this.putObj('tree', te.encode(JSON.stringify(entries)));
    };
    return store(rootNode);
  }

  async readTree(hash, prefix = '', out = {}) {
    const { body } = await this.getObj(hash);
    for (const e of JSON.parse(td.decode(body))) {
      const path = prefix ? `${prefix}/${e.name}` : e.name;
      if (e.kind === 'blob') out[path] = td.decode((await this.getObj(e.hash)).body);
      else await this.readTree(e.hash, path, out);
    }
    return out;
  }

  // ---- commits (single branch `main` in the browser v1) ---------------------

  async head() {
    const b = await this._read('refs/heads/main');
    return b ? td.decode(b).trim() : null;
  }

  async commitObj(hash) {
    return JSON.parse(td.decode((await this.getObj(hash)).body));
  }

  async commit(message, snap, author = 'browser') {
    const tree = await this.writeTree(snap);
    const parent = await this.head();
    let n = 1;
    const parents = [];
    if (parent) {
      const pc = await this.commitObj(parent);
      if (pc.tree === tree) throw new Error('変更がありません(nothing to commit)');
      parents.push(parent);
      n = pc.n + 1;
    }
    const body = JSON.stringify({ tree, parents, author, message, n });
    const hash = await this.putObj('commit', te.encode(body));
    await this._write('refs/heads/main', te.encode(`${hash}\n`));
    return { hash, n };
  }

  async log() {
    const out = [];
    let cur = await this.head();
    while (cur) {
      const c = await this.commitObj(cur);
      out.push({ hash: cur, ...c });
      cur = c.parents[0] ?? null;
    }
    return out;
  }

  async snapshotOf(hash) {
    return this.readTree((await this.commitObj(hash)).tree);
  }
}
