// probe.mjs - determine which hash algorithm the MPQ hash table uses.
// Tables are stored at the END of the file; we only read header + tail.
import { statSync, openSync, readSync, closeSync } from 'node:fs';

const FILE = process.argv[2] || 'C:/Users/larit/Documents/WOW/InfinityCraft_3.3.5a/Data/enUS/patch-enUS.MPQ';
const total = statSync(FILE).size;

const hd = Buffer.alloc(64);
{
  const fd = openSync(FILE, 'r');
  readSync(fd, hd, 0, 64, 0);
  closeSync(fd);
}
const headerSize = hd.readUInt32LE(4);
const fileSize = hd.readUInt32LE(8);
const hashOff = hd.readUInt32LE(0x10);
const blockOff = hd.readUInt32LE(0x14);
const hashCount = hd.readUInt32LE(0x18);
const blockCount = hd.readUInt32LE(0x1c);
console.log('fileSize', total, '(hdr', fileSize, ') headerSize', headerSize);
console.log('hashOff', hashOff, 'blockOff', blockOff, 'hashCount', hashCount, 'blockCount', blockCount);
console.log('hashOff+hashCount*16 =', hashOff + hashCount * 16);
console.log('blockOff+blockCount*16 =', blockOff + blockCount * 16, '== total?', blockOff + blockCount * 16 === total);

function readAt(abs, n) {
  const buf = Buffer.alloc(n);
  const fd = openSync(FILE, 'r');
  readSync(fd, buf, 0, n, abs);
  closeSync(fd);
  return buf;
}

const hCount = Math.min(hashCount, 4 * 1000 * 1000);
const hBuf = readAt(hashOff, hCount * 16);
console.log('\n-- first 8 hash entries --');
for (let i = 0; i < Math.min(8, hCount); i++) {
  const o = i * 16;
  console.log(i, hBuf.toString('hex', o, o + 16),
    'f0=0x' + hBuf.readUInt32LE(o).toString(16),
    'f1=0x' + hBuf.readUInt32LE(o + 4).toString(16),
    'bIdx=' + hBuf.readUInt32LE(o + 8).toString());
}

const bCount = Math.min(blockCount, 4 * 1000 * 1000);
const bBuf = readAt(blockOff, bCount * 16);
console.log('\n-- first 8 block entries --');
for (let i = 0; i < Math.min(8, bCount); i++) {
  const o = i * 16;
  console.log(i, bBuf.toString('hex', o, o + 16),
    'pageOff=0x' + bBuf.readUInt32LE(o).toString(16),
    'comp=' + bBuf.readUInt32LE(o + 4),
    'uncomp=' + bBuf.readUInt32LE(o + 8),
    'flags@12=' + bBuf[o + 12], 'flags@13=' + bBuf[o + 13]);
}

const entries = [];
for (let i = 0; i < hCount; i++) {
  const o = i * 16;
  entries.push({ f0: hBuf.readUInt32LE(o), f1: hBuf.readUInt32LE(o + 4), bIdx: hBuf.readUInt32LE(o + 8) });
}
console.log('\ntotal hash entries:', hCount, ' non-zero f0:', entries.filter((e) => e.f0 !== 0).length, ' non-zero bIdx:', entries.filter((e) => e.bIdx !== 0).length);

const algos = {
  w2c: (s) => { let h = 0; for (let i = 0; i < s.length; i++) { const c = s.charCodeAt(i); if (c === 0x2e) break; h = (((h << 5) + (h >>> 27)) + c) >>> 0; } h = (h + 0x24c1e964) >>> 0; return { mask: h & 0xfffffffd, number: (h >>> 2) & 0x3f }; },
  cnc131: (s) => { let h = 0; for (let i = 0; i < s.length; i++) h = ((h * 131) + s.charCodeAt(i)) >>> 0; return { mask: h & 0xfffffffd, number: (h >>> 2) & 0x3f }; },
  cnc131final: (s) => { let h = 0; for (let i = 0; i < s.length; i++) h = ((h * 131) + s.charCodeAt(i)) >>> 0; h = (h + 0x24c1e964) >>> 0; return { mask: h & 0xfffffffd, number: (h >>> 2) & 0x3f }; },
  mp131: (s) => { let h = 0; for (let i = 0; i < s.length; i++) h = (((h << 7) ^ (h >>> 25)) + s.charCodeAt(i)) >>> 0; return { mask: h & 0xfffffffd, number: (h >>> 2) & 0x3f }; },
  mp131x3: (s) => { let h = 0; for (let i = 0; i < s.length; i++) h = (((h << 7) ^ (h >>> 25)) + s.charCodeAt(i)) >>> 0; h = (h + (h << 3)) >>> 0; return { mask: h & 0xfffffffd, number: (h >>> 2) & 0x3f }; },
  wow3: (s) => { let h = 0; for (let i = 0; i < s.length; i++) h = ((h << 5) + (h << 7) + (h << 31) + s.charCodeAt(i)) >>> 0; const number = ((h & 0x0f) + ((h & 0x70c8) >>> 6) + ((h & 0xf0000000) >>> 22) + ((h & 0x0f000000) >>> 16) + ((h & 0x00c80000) >>> 10)) & 0xff; return { mask: h & 0xfffffffd, number }; },
};

const names = ['itemenUS.dbc', 'item.dbc', 'itemset.dbc', 'spell.dbc', 'itemproperties.dbc', 'wow.exe', 'skillline.dbc', 'faction.dbc', 'area_table.dbc'];
for (const [an, f] of Object.entries(algos)) {
  for (const n of names) {
    const { mask, number } = f(n.toLowerCase());
    const hit = entries.find((e) => e.f0 === mask && (e.f1 & 0x7ff) === number);
    if (hit) console.log('HIT', an, n, 'mask=0x' + mask.toString(16), 'num=' + number, 'block', hit.bIdx, 'rawF1=0x' + hit.f1.toString(16));
  }
}
console.log('\nDone. (No HIT above = none of these algos match this table.)');
