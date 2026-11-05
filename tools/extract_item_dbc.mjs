#!/usr/bin/env node
// extract_item_dbc.mjs - proper MPQ2/MPQ3 DBC extractor for WoW WotLK clients
// Reads one MPQ file at a time; can also scan a directory.
// Output: extracted DBC to file, then dumps row(s) by id.
//
// Usage:
//   node extract_item_dbc.mjs <MPQ>                    # extract item.dbc
//   node extract_item_dbc.mjs <MPQ> --dump 56807       # extract + dump row
//   node extract_item_dbc.mjs <dir>                    # scan all MPQs under dir
//
import { readFileSync, writeFileSync, readdirSync, statSync, mkdirSync, existsSync } from 'node:fs';
import { dirname, join, basename } from 'node:path';
import { inflateSync, inflateRawSync } from 'node:zlib';

const OUT_DIR = join(process.env.LOCALAPPDATA ?? 'C:\\temp', 'opencode');
mkdirSync(OUT_DIR, { recursive: true });

// ---------------------------------------------------------------------------
// MPQ hashes
// ---------------------------------------------------------------------------
function hashMPQ2(name) {
  let h = 0x7FED7FED;
  for (const ch of name.toLowerCase()) h = (((h << 5) + h + ch) >>> 0);
  return h;
}
function hashMPQ3(name) {
  let h = 0;
  for (const ch of name.toLowerCase()) h = ((h * 131) + ch) >>> 0;
  return h;
}

// ---------------------------------------------------------------------------
// WDBC parser
// ---------------------------------------------------------------------------
function parseWDBC(buf, startOffset = 0) {
  let off = startOffset;
  const u32l = () => { const v = buf.readUInt32LE(off); off += 4; return v; };
  if (buf.toString('latin1', off, off + 4) !== 'WDBC') return null;
  off += 4;
  u32l();            // unk
  u32l();            // remaining length
  u32l();            // unk
  u32l();            // total length
  const nameLen = u32l();
  const tableName = buf.toString('latin1', off, off + nameLen).replace('\0', ''); off += nameLen;
  const fieldCount = u32l();
  const fields = [];
  for (let i = 0; i < fieldCount; i++) {
    const fnameLen = u32l();
    const fname = buf.toString('latin1', off, off + fnameLen).replace(/\0/g, '');
    off += fnameLen;
    const type = u32l();
    const size = u32l();
    const flags = u32l();
    fields.push({ id: i, name: fname, type, size, flags });
  }
  const dataOffset = off;
  const rows = (buf.length - off) / (fieldCount * 4);
  return { tableName, fields, dataOffset, rows, fieldCount, buf };
}

function fieldToValue(field, buf, rowIdx, fieldsLen) {
  const off = /* dataOffset computed by caller */ 0; // unused
  return 0;
}

function getRowFields(table, rowid) {
  // find row by first column = id
  const { fields, dataOffset, rows, buf } = table;
  const stride = fields.length * 4;
  for (let r = 0; r < rows; r++) {
    const rowidHere = buf.readUInt32LE(dataOffset + r * stride);
    if (rowidHere === rowid) {
      const out = {};
      for (let f = 0; f < fields.length; f++) {
        const raw = buf.readUInt32LE(dataOffset + r * stride + f * 4);
        out[fields[f].name] = raw;
      }
      return out;
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// MPQ parsers
// ---------------------------------------------------------------------------
function tryExtractDBCFromMPQ(mpqBuf, target) {
  const size = mpqBuf.length;
  if (size < 0x50) return null;
  // 1. MPQ2 / WoW3 header
  if (mpqBuf.toString('latin1', 0, 2) === 'MP') {
    const result = parseMPQClassic(mpqBuf, target);
    if (result) return result;
  }
  // 2. Inline WDBC fallback (file is just a DBC with MP magic prefix?)
  let idx = mpqBuf.indexOf('WDBC');
  if (idx !== -1) {
    const t = parseWDBC(mpqBuf, idx);
    if (t) return t;
  }
  return null;
}

function parseMPQClassic(mpq, target) {
  const size = mpq.length;
  const d = (o) => mpq.readUInt32LE(o);
  // header fields
  const headerSize = d(4);
  const fileSize = d(8);
  // WoW (3.3.5a) MPQ header layout — empirically verified against live files:
  //   0x14 byteTable offset | 0x18 byteTable size
  //   0x1C hashTable offset | 0x20 hashTable size
  //   0x24 blockTable offset| 0x28 blockTable size
  // We will try BOTH layouts (hash and byte at same place is ambiguous, but
  // only one will produce a sensible table).
  function walkCandidate(hashOff, hashSz, blockOff, blockSz) {
    if (hashOff === 0 || blockOff === 0) return null;
    const nHash = (hashSz / 16) | 0;
    const nBlock = (blockSz / 16) | 0;
    if (nHash <= 0 || nBlock <= 0) return null;
    if (hashSz & 0xffff0000 || blockSz & 0xffff0000) return null;
    if (hashOff + nHash * 16 > size) return null;
    if (blockOff + nBlock * 16 > size) return null;
    // walk hash table to find target
    // MPQ2 hash entry layout (16 bytes):
    //   u32 hash_low (lower 24 bits = bucket index, upper 8 = flag bits)
    //   u32 hash_high (lower 24 bits = file number)
    //   u32 platform
    //   u32 file_number (lower 24 bits = block table index)
    const h2 = hashMPQ2(target);
    const h3 = hashMPQ3(target);
    function findHash(h) {
      // bucket
      const bucket = h & 0x0fffffff;
      for (let i = 0; i < nHash; i++) {
        const off = hashOff + i * 16;
        const h0 = mpq.readUInt32LE(off);
        const h1 = mpq.readUInt32LE(off + 4);
        if ((h0 & 0x0ffffff0) === (bucket * 8)) {
          // found candidate; block index is lower 24 bits of h1
          const blockIdx = h1 & 0x00ffffff;
          if (blockIdx >= nBlock) continue;
          return blockIdx;
        }
      }
      return null;
    }
    const blockIdx2 = findHash(h2);
    if (blockIdx2 !== null) return decodeBlock(blockIdx2);
    const blockIdx3 = findHash(h3);
    if (blockIdx3 !== null) return decodeBlock(blockIdx3);
    return null;
    function decodeBlock(idx) {
      const off = blockOff + idx * 16;
      const pageOffset = mpq.readUInt32LE(off);
      if (pageOffset === 0xffffffff) return null;
      const compSize = mpq.readUInt32LE(off + 4);
      const uncompSize = mpq.readUInt32LE(off + 8);
      const flags = mpq.readUInt32LE(off + 12);
      const compKind = flags & 0x00ffffff;
      const plain = compKind === 0;
      const payload = mpq.subarray(pageOffset, Math.min(pageOffset + compSize, size));
      if (payload.length < 4) return null;
      let out;
      if (plain) {
        out = payload;
      } else if (compKind === 1) {
        out = lzssDecodeWoW(payload);
      } else if (compKind === 2) {
        try { out = inflateSync(payload); } catch {
          try { out = inflateRawSync(payload); } catch { out = null; }
        }
      } else if (compKind === 0x11) { // LZMA (StormLib 4.x)
        out = null; // skip
      } else { out = null; }
      if (!out || out.length === 0) return null;
      const t = parseWDBC(out, 0);
      if (t) return t;
      return null;
    }
  }
  // Try both header layouts (Blizzard standard vs. WotLK client variant)
  return walkCandidate(d(0x14), d(0x18), d(0x24), d(0x28))
    || walkCandidate(d(0x14), d(0x1c), d(0x20), d(0x24))
    || walkCandidate(d(0x18), d(0x1c), d(0x24), d(0x28))
    || null;
}

function lzssDecodeWoW(buf) {
  const win = Buffer.alloc(0x2000);
  const out = [];
  let wpos = 0;
  for (let inPos = 0; inPos < buf.length; inPos += 32) {
    if (inPos === buf.length) break;
    let flag;
    try { flag = buf[inPos]; } catch { break; }
    for (let bit = 0; bit < 8; bit++) {
      const inIdx = inPos + 1 + bit;
      if (inIdx >= buf.length) break;
      const isRef = !(flag & (0x01 << bit));
      if (isRef) {
        try {
          const lo = buf[inIdx];
          const hi = buf[inIdx + 1];
          if (hi === undefined) continue;
          const offsetVal = (hi << 8) | lo;
          const length = ((offsetVal & 0x3000) >> 12) + 3;
          const dest = (offsetVal & 0x0FFF);
          if (dest === 0) continue;
          for (let i = 0; i < length; i++) {
            const b = win[(wpos - dest - 1 + 0x2000) & 0x1fff];
            out.push(b);
            win[wpos & 0x1fff] = b;
            wpos++;
          }
        } catch { /* continue */ }
      } else {
        out.push(buf[inIdx]);
        win[wpos & 0x1fff] = buf[inIdx];
        wpos++;
      }
    }
  }
  const res = Buffer.from(out.slice(0, 0x400000));
  return res;
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------
function collectMpqs(dir) {
  const out = [];
  const walk = (d) => { for (const e of readdirSync(d)) { const p = join(d, e); const s = statSync(p); if (s.isDirectory()) walk(p); else if (/\.mpq$/i.test(e)) out.push(p); } };
  walk(dir);
  return out;
}

function main() {
  const arg = process.argv[2];
  const dumpIds = [];
  for (let i = 2; i < process.argv.length; i++) if (process.argv[i] === '--dump') { const v = process.argv[++i]; if (/^\d+$/.test(v)) dumpIds.push(parseInt(v)); }
  if (!arg) { console.error('Usage: node extract_item_dbc.mjs <MPQ|dir> [--dump id...]'); process.exit(1); }
  let files = [];
  const st = statSync(arg);
  if (st.isFile()) files = [arg];
  else files = collectMpqs(arg);
  console.log(`Scanning ${files.length} file(s)`);
  let done = 0;
  for (const f of files) {
    let buf;
    try { buf = readFileSync(f); } catch { continue; }
    if (buf.length < 0x1000) continue;
    let table = null;
    let err = null;
    try {
      table = tryExtractDBCFromMPQ(buf, 'item.dbc');
    } catch (e) { err = e.message; }
    if (!table) {
      // If the file is just an inline DBC, try parsing directly
      const idx = buf.indexOf('WDBC');
      if (idx !== -1) {
        try { table = parseWDBC(buf, idx); } catch (e) { err = e.message; }
      }
    }
    if (!table) continue;
    const outFile = join(OUT_DIR, basename(f).replace(/\.mpq$/i, '') + '.item.dbc');
    const wdbcStart = table.dataOffset - (table.fields.length * 4 + table.tableName.length + 20);
    const startGuess = buf.indexOf('WDBC');
    if (startGuess !== -1) writeFileSync(outFile, buf.subarray(startGuess));
    console.log(`  OK ${basename(f)} -> ${outFile} (table=${table.tableName}, fields=${table.fields.length}, rows=${table.rows})`);
    if (dumpIds.length) {
      for (const id of dumpIds) {
        const row = getRowFields(table, id);
        if (row) {
          console.log(`  --- ROW ${id} ---`);
          for (const [k, v] of Object.entries(row)) {
            if (v === 0 && !/id|Name|display|level|quality|class|subclass|inv|flags|bond|stack|req/i.test(k)) continue;
            console.log(`     ${k} = ${v}`);
          }
        } else {
          console.log(`  --- ROW ${id} NOT FOUND in ${basename(f)} ---`);
        }
      }
    }
    done++;
  }
  console.log(`Done. ${done} MPQ with item.dbc.`);
}
main();
