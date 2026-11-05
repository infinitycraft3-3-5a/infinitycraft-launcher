// findmpq.mjs - read-only MPQ scanner: locate DBC files inside *.MPQ archives.
// Zero dependencies. Implements MPQ2 (WoW/W2C) and MPQ3 (Cata) hash algorithms.

import { readdir, open, stat } from 'node:fs/promises';
import path from 'node:path';

const DEFAULT_ROOT = 'C:/Users/larit/Documents/WOW/InfinityCraft_3.3.5a/Data';
const TARGETS = [
  'item.dbc',
  'itemenUS.dbc',
  'itemrandomsuffix.dbc',
  'itemrandomproperties.dbc',
  'itemproperties.dbc',
  'itemextendedcost.dbc',
  'itemlimitcategory.dbc',
  'itemset.dbc',
  'itemsubclass.dbc',
];

function parseArgs(argv) {
  const opts = { root: DEFAULT_ROOT, maxMB: 0, targets: TARGETS };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith('--root=')) opts.root = a.slice(7);
    else if (a === '--root') opts.root = argv[++i];
    else if (a.startsWith('--maxSize=')) opts.maxMB = parseFloat(a.slice(10));
    else if (a === '--maxSize') opts.maxMB = parseFloat(argv[++i]);
    else if (a.startsWith('--targets=')) opts.targets = a.slice(10).split(',').map((s) => s.trim().toLowerCase()).filter(Boolean);
    else if (a === '--targets') opts.targets = argv[++i].split(',').map((s) => s.trim().toLowerCase()).filter(Boolean);
  }
  return opts;
}

// W2C hash (MPQEditor/StormLib "type 2"): h = (h<<5)+(h>>27)+c, then +0x24C1E964
function hashW2C(name) {
  const s = name.toLowerCase();
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    const ch = s.charCodeAt(i);
    if (ch === 0x2e) break;
    h = (((h << 5) + (h >>> 27)) + ch) >>> 0;
  }
  h = (h + 0x24c1e964) >>> 0;
  return { number: (h >>> 2) & 0x3f, mask: h & 0xfffffffd };
}

// C&C / MPQ3 hash (MPQ Editor "type 3"): h = h*131 + c
function hashCnC(name) {
  const s = name.toLowerCase();
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = ((h * 131) + s.charCodeAt(i)) >>> 0;
  }
  return { number: (h >>> 2) & 0x3f, mask: h & 0xfffffffd };
}

const MAX_HASH_TABLE_ENTRIES = 4_000_000;

async function scanMpq(file, size, targets) {
  if (size < 64) return { file, error: 'too small' };
  const out = {};
  for (const t of targets) out[t] = null;

  const fh = await open(file, 'r');
  try {
    const header = Buffer.alloc(64);
    await fh.read(header, 0, 64, 0);
    if (header.toString('ascii', 0, 2) !== 'MP') return { file, error: 'not an MPQ file (bad magic)' };
    // MPQ header: byteTable@0x14, hashTable@0x1C, blockTable@0x24 (offsets+sizes, 4 bytes each)
    const hashTableOffset = header.readUInt32LE(0x1c);
    const hashTableSize = header.readUInt32LE(0x20);
    const blockTableOffset = header.readUInt32LE(0x24);
    const blockTableSize = header.readUInt32LE(0x28);
    const hashCount = (hashTableSize / 16) >>> 0;
    const blockCount = (blockTableSize / 16) >>> 0;

    const htEntries = Math.min(hashCount, MAX_HASH_TABLE_ENTRIES);
    if (htEntries <= 0 || hashTableOffset >= size) return { file, error: 'invalid hash table header' };
    const htBuf = Buffer.alloc(htEntries * 16);
    await fh.read(htBuf, 0, htEntries * 16, hashTableOffset);

    const lookup = (needNumber, needMask) => {
      for (let i = 0; i < htEntries; i++) {
        if ((htBuf.readUInt32LE(i * 16) & 0xfffffffd) === needMask && (htBuf.readUInt32LE(i * 16 + 4) & 0xffffff) === needNumber) {
          return htBuf.readUInt32LE(i * 16 + 8) & 0xffffff;
        }
      }
      return null;
    };

    for (const t of targets) {
      let found = null;
      for (const [algo, label] of [[hashW2C, 'W2C'], [hashCnC, 'CnC']]) {
        const { number, mask } = algo(t);
        const blockIdx = lookup(number, mask);
        if (blockIdx !== null && blockIdx < blockCount) {
          if (blockTableOffset + (blockIdx + 1) * 16 <= size) {
            const be = Buffer.alloc(16);
            await fh.read(be, 0, 16, blockTableOffset + blockIdx * 16);
            const pageOffset = be.readUInt32LE(0);
            const compSize = be.readUInt32LE(4);
            const uncompSize = be.readUInt32LE(8);
            if (pageOffset !== 0xffffffff && pageOffset > 0 && compSize > 0 &&
                pageOffset + compSize <= size) {
              found = { offset: pageOffset, compSize, uncompSize, algo: label };
            }
          }
        }
        if (found) break;
      }
      out[t] = found;
    }
    return { file, results: out };
  } finally {
    await fh.close();
  }
}

async function collectMpqFiles(dir) {
  const results = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const abs = path.join(dir, entry.name);
    if (entry.isDirectory()) results.push(...(await collectMpqFiles(abs)));
    else if (/\.mpq$/i.test(entry.name)) {
      const st = await stat(abs);
      results.push({ file: abs, size: st.size });
    }
  }
  return results;
}

const WOTLK_LOAD_ORDER = [
  /(enUS)?\/base-enUS\.MPQ$/i,
  /(enUS)?\/expansion-locale-enUS\.MPQ$/i,
  /(enUS)?\/patch-enUS\.MPQ$/i,
  /(enUS)?\/patch-enUS-2\.MPQ$/i,
  /(enUS)?\/patch-enUS-3\.MPQ$/i,
  /(enUS)?\/lichking-locale-enUS\.MPQ$/i,
  /(enUS)?\/speech-enUS\.MPQ$/i,
  (f) => f.match(/expansion\.MPQ$/i),
  (f) => f.match(/patch(-\d)?\.MPQ$/i),
  (f) => f.match(/lichking\.MPQ$/i),
  (f) => f.match(/patch-[A-Z]\.mpq$/i),
];

function loadRank(file) {
  const name = path.basename(file);
  for (let i = 0; i < WOTLK_LOAD_ORDER.length; i++) {
    const p = WOTLK_LOAD_ORDER[i];
    const ok = typeof p === 'function' ? p(name) : p.test(name);
    if (ok) {
      const patchIdx = name.match(/patch-(\d)\.MPQ$/i) ? parseInt(RegExp.$1, 10) * 10 : 0;
      return i * 100 + patchIdx;
    }
  }
  return 10000;
}

function fmt(n) {
  return n.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',');
}

function toHex(n) {
  return '0x' + n.toString(16).toUpperCase().padStart(8, '0');
}

async function main() {
  const opts = parseArgs(process.argv);
  console.log(`Root    : ${opts.root}`);
  console.log(`Targets : ${opts.targets.join(', ')}`);
  console.log('');

  const mpqs = (await collectMpqFiles(opts.root)).sort((a, b) => a.file.localeCompare(b.file));
  if (opts.maxMB > 0) {
    const maxB = opts.maxMB * 1048576;
    mpqs.splice(0, mpqs.length, ...mpqs.filter((m) => m.size <= maxB));
  }
  if (mpqs.length === 0) {
    console.log('No MPQ files found. Aborting.');
    process.exit(1);
  }
  console.log(`MPQs    : ${mpqs.length}\n`);

  const perFile = [];
  for (const m of mpqs) {
    process.stdout.write(`  scanning ${path.relative(opts.root, m.file)} ... `);
    const r = await scanMpq(m.file, m.size, opts.targets);
    if (r.error) {
      console.log(`SKIP (${r.error})`);
      continue;
    }
    const hits = opts.targets.filter((t) => r.results[t]);
    if (hits.length === 0) {
      console.log('no targets found');
      continue;
    }
    console.log(`${hits.length} match(es)`);
    for (const t of hits) {
      const rr = r.results[t];
      console.log(`    ${t.padEnd(24)} offset=${toHex(rr.offset)}  size=${fmt(rr.compSize)} (${fmt(rr.uncompSize)} uncomp)  [${rr.algo}]`);
    }
    perFile.push({ file: m.file, results: r.results });
  }

  if (perFile.length === 0) {
    console.log('\nNo DBC was found in any scanned MPQ.');
    process.exit(2);
  }

  console.log('\n== SUMMARY (topmost MPQ in WotLK load order wins) ==');
  for (const t of opts.targets) {
    const holders = perFile
      .filter((p) => p.results[t])
      .sort((a, b) => loadRank(b.file) - loadRank(a.file));
    if (holders.length === 0) {
      console.log(`  ${t.padEnd(26)} NOT FOUND in any MPQ`);
    } else {
      const top = holders[0];
      const rr = top.results[t];
      console.log(`  ${t.padEnd(26)} ${path.relative(opts.root, top.file).padEnd(28)} offset=${toHex(rr.offset)}  size=${fmt(rr.compSize)}`);
      for (let i = 1; i < holders.length; i++) {
        console.log(`  (also in   )            ${path.relative(opts.root, holders[i].file)}`);
      }
    }
  }
  console.log('\nOpen the "top" MPQ in MPQEditor/WDBX, export the DBC, edit, replace, re-publish.');
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
