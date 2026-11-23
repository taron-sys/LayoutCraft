// A small ZIP writer for the shell's "Export all pages": files stored without compression (SVG text and PNG are small or already compressed),
// each with its CRC-32, a central directory and an end record. No library; no ZIP64 (up to 65535 files and 4 GB, which the export never reaches).
const encoder = new TextEncoder();

const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

/** CRC-32 (the checksum ZIP files carry) of `bytes`. */
export function crc32(bytes) {
  let c = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) c = CRC_TABLE[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

/**
 * files: [{name, bytes: Uint8Array}] (names unique, '/' separated, UTF-8)
 * Returns the ZIP archive as a Uint8Array.
 */
export function buildZip(files, date = new Date()) {
  if (files.length > 65535) throw new Error('A ZIP file holds at most 65535 files');
  const seen = new Set();
  const stamp = {
    time: (date.getHours() << 11) | (date.getMinutes() << 5) | (date.getSeconds() >> 1),
    day: ((Math.max(1980, date.getFullYear()) - 1980) << 9) | ((date.getMonth() + 1) << 5) | date.getDate(),
  };
  const parts = [];
  const central = [];
  let offset = 0;
  const push = (bytes) => { parts.push(bytes); offset += bytes.length; };
  for (const file of files) {
    if (seen.has(file.name)) throw new Error(`Two files are called ${file.name}`);
    seen.add(file.name);
    const name = encoder.encode(file.name);
    if (name.length > 65535) throw new Error('A file name is too long');
    if (file.bytes.length >= 0xffffffff || offset >= 0xffffffff) throw new Error('The ZIP file would be larger than 4 GB');
    const crc = crc32(file.bytes);
    const local = new DataView(new ArrayBuffer(30));
    local.setUint32(0, 0x04034b50, true);
    local.setUint16(4, 20, true); // version needed
    local.setUint16(6, 0x0800, true); // flags: the name is UTF-8
    local.setUint16(8, 0, true); // stored
    local.setUint16(10, stamp.time, true);
    local.setUint16(12, stamp.day, true);
    local.setUint32(14, crc, true);
    local.setUint32(18, file.bytes.length, true);
    local.setUint32(22, file.bytes.length, true);
    local.setUint16(26, name.length, true);
    local.setUint16(28, 0, true);
    const start = offset;
    push(new Uint8Array(local.buffer));
    push(name);
    push(file.bytes);
    const entry = new DataView(new ArrayBuffer(46));
    entry.setUint32(0, 0x02014b50, true);
    entry.setUint16(4, 20, true); // version made by
    entry.setUint16(6, 20, true); // version needed
    entry.setUint16(8, 0x0800, true);
    entry.setUint16(10, 0, true);
    entry.setUint16(12, stamp.time, true);
    entry.setUint16(14, stamp.day, true);
    entry.setUint32(16, crc, true);
    entry.setUint32(20, file.bytes.length, true);
    entry.setUint32(24, file.bytes.length, true);
    entry.setUint16(28, name.length, true);
    entry.setUint32(42, start, true);
    central.push(new Uint8Array(entry.buffer), name);
  }
  const directoryStart = offset;
  for (const bytes of central) push(bytes);
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true);
  end.setUint16(8, files.length, true);
  end.setUint16(10, files.length, true);
  end.setUint32(12, offset - directoryStart, true);
  end.setUint32(16, directoryStart, true);
  push(new Uint8Array(end.buffer));
  const out = new Uint8Array(offset);
  let at = 0;
  for (const bytes of parts) { out.set(bytes, at); at += bytes.length; }
  return out;
}
