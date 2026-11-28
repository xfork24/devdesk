// A ZIP writer without dependencies. Entries are stored uncompressed with a fixed timestamp
// and written in name order, so the same package always produces the same archive.
import { crc32, deflateRawSync, inflateRawSync } from "node:zlib";

const FIXED_TIME = 0; // 00:00:00
const FIXED_DATE = 0x21; // 1980-01-01, the earliest date DOS can encode
const UTF8 = 0x0800;

export function zip(entries) {
  const names = Object.keys(entries).sort();
  const local = [];
  const directory = [];
  let offset = 0;
  for (const name of names) {
    const encoded = Buffer.from(name, "utf8");
    const raw = Buffer.isBuffer(entries[name])
      ? entries[name]
      : Buffer.from(entries[name]);
    const data = deflateRawSync(raw, { level: 9 });
    const checksum = crc32(raw);
    const header = Buffer.alloc(30);
    header.writeUInt32LE(0x04034b50, 0);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(UTF8, 6);
    header.writeUInt16LE(8, 8); // deflate
    header.writeUInt16LE(FIXED_TIME, 10);
    header.writeUInt16LE(FIXED_DATE, 12);
    header.writeUInt32LE(checksum, 14);
    header.writeUInt32LE(data.length, 18);
    header.writeUInt32LE(raw.length, 22);
    header.writeUInt16LE(encoded.length, 26);
    local.push(header, encoded, data);

    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(20, 4); // made by
    entry.writeUInt16LE(20, 6); // needed to read
    entry.writeUInt16LE(UTF8, 8);
    entry.writeUInt16LE(8, 10);
    entry.writeUInt16LE(FIXED_TIME, 12);
    entry.writeUInt16LE(FIXED_DATE, 14);
    entry.writeUInt32LE(checksum, 16);
    entry.writeUInt32LE(data.length, 20);
    entry.writeUInt32LE(raw.length, 24);
    entry.writeUInt16LE(encoded.length, 28);
    entry.writeUInt32LE(offset, 42);
    directory.push(entry, encoded);
    offset += header.length + encoded.length + data.length;
  }
  const central = Buffer.concat(directory);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(names.length, 8);
  end.writeUInt16LE(names.length, 10);
  end.writeUInt32LE(central.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...local, central, end]);
}

// Reads the entries of an archive written by this writer or by any other ZIP tool.
export function unzip(bytes) {
  let end = -1;
  for (let at = bytes.length - 22; at >= Math.max(0, bytes.length - 65557); at--)
    if (bytes.readUInt32LE(at) === 0x06054b50) {
      end = at;
      break;
    }
  if (end < 0) throw new Error("不是 ZIP 文件");
  const count = bytes.readUInt16LE(end + 10);
  let at = bytes.readUInt32LE(end + 16);
  const entries = {};
  for (let _ = 0; _ < count; _++) {
    if (bytes.readUInt32LE(at) !== 0x02014b50) throw new Error("ZIP 目录损坏");
    const method = bytes.readUInt16LE(at + 10);
    const size = bytes.readUInt32LE(at + 20);
    const nameLength = bytes.readUInt16LE(at + 28);
    const extraLength = bytes.readUInt16LE(at + 30);
    const commentLength = bytes.readUInt16LE(at + 32);
    const local = bytes.readUInt32LE(at + 42);
    const name = bytes.subarray(at + 46, at + 46 + nameLength).toString("utf8");
    const start = local + 30 + bytes.readUInt16LE(local + 26) + bytes.readUInt16LE(local + 28);
    const data = bytes.subarray(start, start + size);
    entries[name] = method === 0 ? data : method === 8 ? inflateRawSync(data) : null;
    if (entries[name] === null) throw new Error(`ZIP 条目 ${name} 使用了不支持的压缩方式`);
    at += 46 + nameLength + extraLength + commentLength;
  }
  return entries;
}
