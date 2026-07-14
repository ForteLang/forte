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

  // ---- branches & commits (mirrors the CLI's HEAD/refs model) ---------------

  async _remove(path) {
    try {
      const { dir, base } = await this._resolve(path, false);
      await dir.removeEntry(base);
    } catch { /* absent is fine */ }
  }

  /// Branch name HEAD points at (repos from the single-branch era default
  /// to main).
  async headRef() {
    const b = await this._read('HEAD');
    if (!b) return 'main';
    const s = td.decode(b).trim();
    return s.startsWith('ref: ') ? s.slice(5) : null; // null = detached
  }

  async branchHash(name) {
    const b = await this._read(`refs/heads/${name}`);
    return b ? td.decode(b).trim() : null;
  }

  async head() {
    const ref = await this.headRef();
    if (ref) return this.branchHash(ref);
    return td.decode(await this._read('HEAD')).trim();
  }

  async branches() {
    const out = [];
    try {
      const heads = await (await this.dir.getDirectoryHandle('refs')).getDirectoryHandle('heads');
      for await (const [name, handle] of heads.entries()) {
        if (handle.kind === 'file') out.push({ name, hash: await this.branchHash(name) });
      }
    } catch { /* no refs yet */ }
    return out.sort((a, b) => a.name.localeCompare(b.name));
  }

  /// Create `name` at the current head and switch HEAD to it (checkout -b).
  async createBranch(name) {
    if (!/^[\w./-]+$/.test(name)) throw new Error(`ブランチ名に使えない文字があります: ${name}`);
    if (await this.branchHash(name)) throw new Error(`ブランチ ${name} は既にあります`);
    const head = await this.head();
    if (!head) throw new Error('まだコミットがありません');
    await this._write(`refs/heads/${name}`, te.encode(`${head}\n`));
    await this._write('HEAD', te.encode(`ref: ${name}\n`));
    return head;
  }

  /// Point HEAD at a branch and return its snapshot for the caller to
  /// restore into the working tree. The caller checks cleanliness first.
  async checkout(name) {
    const hash = await this.branchHash(name);
    if (!hash) throw new Error(`ブランチ ${name} がありません`);
    await this._write('HEAD', te.encode(`ref: ${name}\n`));
    return this.snapshotOf(hash);
  }

  async mergeHead() {
    const b = await this._read('MERGE_HEAD');
    return b ? td.decode(b).trim() : null;
  }

  async commitObj(hash) {
    return JSON.parse(td.decode((await this.getObj(hash)).body));
  }

  async commit(message, snap, author = 'browser') {
    const branch = await this.headRef();
    if (!branch) throw new Error('HEAD がブランチを指していません(ブランチに戻ってから commit)');
    const tree = await this.writeTree(snap);
    const parent = await this.branchHash(branch);
    const mergeHead = await this.mergeHead();
    let n = 1;
    const parents = [];
    if (parent) {
      const pc = await this.commitObj(parent);
      if (pc.tree === tree && !mergeHead) throw new Error('変更がありません(nothing to commit)');
      parents.push(parent);
      n = pc.n + 1;
    }
    if (mergeHead) {
      // a conflict resolution finishes the merge: MERGE_HEAD = parent #2
      parents.push(mergeHead);
      n = Math.max(n, (await this.commitObj(mergeHead)).n + 1);
    }
    const body = JSON.stringify({ tree, parents, author, message, n });
    const hash = await this.putObj('commit', te.encode(body));
    await this._write(`refs/heads/${branch}`, te.encode(`${hash}\n`));
    await this._remove('MERGE_HEAD');
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

  // ---- merge (same algorithm as the CLI: fast-forward, file-level 3-way,
  // line-level merge3 with git-style markers, MERGE_HEAD on conflict) --------

  async mergeBase(a, b) {
    const ancestors = new Set();
    let queue = [a];
    while (queue.length) {
      const h = queue.pop();
      if (!ancestors.has(h)) {
        ancestors.add(h);
        queue.push(...(await this.commitObj(h)).parents);
      }
    }
    queue = [b];
    const seen = new Set();
    while (queue.length) {
      const h = queue.shift();
      if (ancestors.has(h)) return h;
      if (!seen.has(h)) {
        seen.add(h);
        queue.push(...(await this.commitObj(h)).parents);
      }
    }
    return null;
  }

  /// Merge branch `other` into the current branch. Returns
  ///   {kind: 'fast-forward'|'merged', snapshot, hash}     on success
  ///   {kind: 'conflict', snapshot, conflicts: [path…]}    with markers left
  /// The caller restores `snapshot` into the working tree in every case.
  async merge(other, author = 'browser') {
    const branch = await this.headRef();
    if (!branch) throw new Error('HEAD がブランチを指していません');
    const oursHash = await this.branchHash(branch);
    if (!oursHash) throw new Error('まだコミットがありません');
    const theirsHash = await this.branchHash(other);
    if (!theirsHash) throw new Error(`ブランチ ${other} がありません`);
    if (oursHash === theirsHash) throw new Error('同じコミットです(マージするものがありません)');
    const baseHash = await this.mergeBase(oursHash, theirsHash);
    if (!baseHash) throw new Error('共通の祖先がありません');
    if (baseHash === theirsHash) throw new Error(`'${other}' は既に取り込み済みです`);
    const theirs = await this.snapshotOf(theirsHash);
    if (baseHash === oursHash) {
      await this._write(`refs/heads/${branch}`, te.encode(`${theirsHash}\n`));
      return { kind: 'fast-forward', snapshot: theirs, hash: theirsHash };
    }
    const base = await this.snapshotOf(baseHash);
    const ours = await this.snapshotOf(oursHash);

    const merged = {};
    const conflicts = [];
    const paths = [...new Set([...Object.keys(base), ...Object.keys(ours), ...Object.keys(theirs)])].sort();
    for (const path of paths) {
      const [b, o, t] = [base[path], ours[path], theirs[path]];
      if (o === t) {
        if (o !== undefined) merged[path] = o;
      } else if (o === b) {
        if (t !== undefined) merged[path] = t;
      } else if (t === b) {
        if (o !== undefined) merged[path] = o;
      } else if (o === undefined || t === undefined) {
        conflicts.push(`${path} (片方で編集、片方で削除)`);
        merged[path] = o ?? t;
      } else {
        const { text, conflicted } = merge3(b ?? '', o, t, branch, other);
        if (conflicted) conflicts.push(`${path} (同じ行を両方で編集)`);
        merged[path] = text;
      }
    }

    if (conflicts.length) {
      await this._write('MERGE_HEAD', te.encode(`${theirsHash}\n`));
      return { kind: 'conflict', snapshot: merged, conflicts };
    }
    const tree = await this.writeTree(merged);
    const n =
      Math.max((await this.commitObj(oursHash)).n, (await this.commitObj(theirsHash)).n) + 1;
    const body = JSON.stringify({
      tree, parents: [oursHash, theirsHash], author, message: `merge ${other}`, n,
    });
    const hash = await this.putObj('commit', te.encode(body));
    await this._write(`refs/heads/${branch}`, te.encode(`${hash}\n`));
    return { kind: 'merged', snapshot: merged, hash };
  }
}

// ---- line-level 3-way merge (a faithful port of the CLI's merge3) ------------

function lcsEdits(base, side) {
  const n = base.length, m = side.length;
  const dp = new Uint32Array((n + 1) * (m + 1));
  const at = (i, j) => i * (m + 1) + j;
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[at(i, j)] =
        base[i] === side[j]
          ? dp[at(i + 1, j + 1)] + 1
          : Math.max(dp[at(i + 1, j)], dp[at(i, j + 1)]);
    }
  }
  const edits = [];
  let i = 0, j = 0, es = null, lines = [];
  while (i < n || j < m) {
    if (i < n && j < m && base[i] === side[j]) {
      if (es !== null) {
        edits.push({ s: es, e: i, lines });
        es = null;
        lines = [];
      }
      i++; j++;
    } else if (j < m && (i === n || dp[at(i, j + 1)] >= dp[at(i + 1, j)])) {
      es ??= i;
      lines.push(side[j]);
      j++;
    } else {
      es ??= i;
      i++;
    }
  }
  if (es !== null) edits.push({ s: es, e: n, lines });
  return edits;
}

function sideRange(base, edits, s, e) {
  const out = [];
  let i = s;
  for (const ed of edits) {
    if (ed.e < s || ed.s > e) continue;
    while (i < ed.s) out.push(base[i++]);
    out.push(...ed.lines);
    i = ed.e;
  }
  while (i < e) out.push(base[i++]);
  return out;
}

export function merge3(base, ours, theirs, oursName, theirsName) {
  const b = base.split('\n');
  if (b[b.length - 1] === '') b.pop(); // lines(), not split: no phantom tail
  const oLines = ours.split('\n'); if (oLines[oLines.length - 1] === '') oLines.pop();
  const tLines = theirs.split('\n'); if (tLines[tLines.length - 1] === '') tLines.pop();
  const eo = lcsEdits(b, oLines);
  const et = lcsEdits(b, tLines);

  const regions = [
    ...eo.map((ed) => ({ s: ed.s, e: Math.max(ed.e, ed.s), ours: true, theirs: false })),
    ...et.map((ed) => ({ s: ed.s, e: Math.max(ed.e, ed.s), ours: false, theirs: true })),
  ].sort((a, z) => a.s - z.s || a.e - z.e);
  const clusters = [];
  for (const r of regions) {
    const last = clusters[clusters.length - 1];
    if (last && r.s <= last.e) {
      last.e = Math.max(last.e, r.e);
      last.ours ||= r.ours;
      last.theirs ||= r.theirs;
    } else {
      clusters.push({ ...r });
    }
  }

  const out = [];
  let conflicted = false;
  let i = 0;
  for (const c of clusters) {
    while (i < Math.min(c.s, b.length)) out.push(b[i++]);
    const e = Math.max(Math.min(c.e, b.length), Math.min(c.s, b.length));
    const o = sideRange(b, eo, Math.min(c.s, b.length), e);
    const t = sideRange(b, et, Math.min(c.s, b.length), e);
    if (c.ours && !c.theirs) out.push(...o);
    else if (!c.ours && c.theirs) out.push(...t);
    else if (o.join('\n') === t.join('\n')) out.push(...o);
    else {
      conflicted = true;
      out.push(`<<<<<<< ${oursName}`, ...o, '=======', ...t, `>>>>>>> ${theirsName}`);
    }
    i = e;
  }
  while (i < b.length) out.push(b[i++]);
  return { text: out.join('\n') + '\n', conflicted };
}
