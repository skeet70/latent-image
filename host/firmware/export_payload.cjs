#!/usr/bin/env node
// Host-only adapter for pinned upstream JavaScript. No upstream CLI, assembler,
// model database, network, camera adapter, or patch compilation is executed.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const vm = require('node:vm');
const cp = require('node:child_process');

const REVISION = '0e81431c32f2fc908d9626d255e1d758ebfe8a79';
const INPUT_SHA256 = '43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490';
const INPUT_SIZE = 38405148;
const hash = data => crypto.createHash('sha256').update(data).digest('hex');

function readRegular(filename) {
  const limit = 64 * 1024 * 1024;
  // Reject special files before opening; O_NONBLOCK prevents a substituted FIFO
  // from blocking before the descriptor check. Never read a non-regular fd.
  const before = fs.lstatSync(filename);
  if (!before.isFile() || before.isSymbolicLink() || before.size > limit) {
    throw Error('input must be a regular file without symlinks <=64 MiB');
  }
  const fd = fs.openSync(filename, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
  try {
    const stat = fs.fstatSync(fd);
    if (!stat.isFile() || stat.size > limit) throw Error('input must be a regular file <=64 MiB');
    // A file growing after fstat cannot cause an unbounded read/allocation.
    const data = Buffer.alloc(stat.size + 1);
    let length = 0;
    while (length < data.length) {
      const count = fs.readSync(fd, data, length, data.length - length, null);
      if (count === 0) break;
      length += count;
    }
    if (length > stat.size) throw Error('input size changed during bounded read');
    return data.subarray(0, length);
  } finally { fs.closeSync(fd); }
}

function main() {
  const [checkout, input, output] = process.argv.slice(2);
  if (!output || process.argv.length !== 5) {
    throw Error('usage: node host/firmware/export_payload.cjs PATCHER_CHECKOUT INPUT_DAT NEW_PRIVATE_OUTPUT_DIRECTORY');
  }
  // Require the existing ignored project directory, resolving parent symlinks.
  // The destination itself must be new, so it cannot alias an existing input.
  const privateRoot = fs.realpathSync(path.join(__dirname, '../../private-data'));
  const parent = fs.realpathSync(path.dirname(path.resolve(output)));
  if (parent !== privateRoot && !parent.startsWith(privateRoot + path.sep)) {
    throw Error('output parent must be inside this worktree private-data directory');
  }
  const destination = path.join(parent, path.basename(output));
  const data = readRegular(input);
  if (data.length !== INPUT_SIZE || hash(data) !== INPUT_SHA256) throw Error('unsupported input: exact X-M1 1.50 hash required');
  // Independently check the exact header framing before calling upstream.
  if (data.readUInt32LE(0) !== 4 || data.readUInt32LE(516) !== 1 ||
      data.readUInt32LE(520) !== 0x50 || data.readUInt32LE(528) !== 1) throw Error('unexpected type/version/device header');

  const inventory = JSON.parse(readRegular(path.join(__dirname, '../../third_party/firmware-source-inventory.json')));
  const source = inventory.sources.find(s => s.revision === REVISION);
  if (!source) throw Error('missing pinned source inventory');
  const texts = [];
  for (const filename of ['util.js', 'patcher.js']) {
    const content = cp.execFileSync('git', ['-C', checkout, 'show', REVISION + ':' + filename], {maxBuffer: 1024 * 1024});
    const expected = source.files.find(f => f.path === filename);
    if (!expected || content.length !== expected.size_bytes || hash(content) !== expected.sha256) {
      throw Error('upstream source hash mismatch: ' + filename);
    }
    texts.push([filename, content.toString('utf8')]);
  }
  const context = vm.createContext({cliMode: true, ui: {log() {}}, alert() {throw Error('upstream endian check failed');}});
  for (const [filename, content] of texts) vm.runInContext(content, context, {filename, timeout: 1000});
  // Expose only the in-memory input; upstream definitions get no host I/O API.
  context.inputBuffer = data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength);
  vm.runInContext('firmware.init(); firmware.size = inputBuffer.byteLength; firmware.reader = {result: inputBuffer}; if (firmware.parse() !== 0) throw Error("unsupported header");', context, {timeout: 1000});
  if (context.firmware.header.size !== 532) throw Error('unexpected upstream header length');
  vm.runInContext('firmware.readable();', context, {timeout: 30000});
  const upstream = Buffer.from(context.firmware.result);
  const payload = upstream.subarray(0, data.length - 532);
  if (upstream.length !== data.length) throw Error('unexpected upstream output length');
  for (let i = 0; i < payload.length; ++i) {
    if (payload[i] !== (data[i + 532] ^ 255)) throw Error('complement cross-check failed');
  }
  if (!upstream.subarray(payload.length).every(b => b === 255)) throw Error('unexpected synthetic padding');
  const files = [
    {name: 'upstream-readable.bin', bytes: upstream, origin: 'Unmodified upstream readable() result; includes 532 synthetic 0xff padding bytes.'},
    {name: 'payload-complement.bin', bytes: payload, origin: 'Unmodified upstream result clipped to exact source payload length; not decompressed or mapped.'},
  ];
  fs.mkdirSync(destination, {mode: 0o700});
  for (const file of files) fs.writeFileSync(path.join(destination, file.name), file.bytes, {flag: 'wx', mode: 0o600});
  const metadata = {
    schema_version: 1,
    scope: 'Exact public X-M1 1.50 update container; installed correspondence unknown; byte complement only.',
    input: {sha256: INPUT_SHA256, size_bytes: data.length},
    source: {revision: REVISION, invoked_files: ['util.js', 'patcher.js'], invoked_functions: ['firmware.init', 'firmware.parse', 'firmware.readable', 'firmware.loadFirmware']},
    tool: {node: process.version, adapter_sha256: hash(fs.readFileSync(__filename))},
    header: {type: 4, code_size_bytes: 512, size_bytes: 532, version: '1.50', checksum_word: '0x' + data.readUInt32LE(524).toString(16), device_type_word: 1},
    checksum_status: 'unverified: upstream parse() records a checksum word but does not validate it',
    segment_map: null, load_addresses: null, architecture: null,
    artifacts: files.map(file => ({path: file.name, sha256: hash(file.bytes), size_bytes: file.bytes.length, origin: file.origin})),
  };
  fs.writeFileSync(path.join(destination, 'inventory.json'), JSON.stringify(metadata, null, 2) + '\n', {flag: 'wx', mode: 0o600});
  process.stdout.write(JSON.stringify(metadata, null, 2) + '\n');
}

try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
