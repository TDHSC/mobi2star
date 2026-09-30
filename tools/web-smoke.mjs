// Smoke test for the browser build, with Node and no npm packages. It loads
// _site/ as the page's worker does and requires every archive to unzip to
// exactly the files `mobi2star convert --profile stardict` writes.
//
//   node tools/web-smoke.mjs CLI [--measure BOOK.mobi]
//
// CLI is a built mobi2star binary. --measure also converts BOOK, prints the
// time and WebAssembly memory, and compares it the same way.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';
import { pathToFileURL } from 'node:url';
import { isDeepStrictEqual } from 'node:util';
import { inflateRawSync } from 'node:zlib';

const [cli, flag, measured] = process.argv.slice(2);
if (!cli || (flag && flag !== '--measure') || (flag && !measured)) {
  console.error('usage: node tools/web-smoke.mjs CLI [--measure BOOK.mobi]');
  process.exit(2);
}
const versions = readdirSync('_site/v');
if (versions.length !== 1) throw new Error(`expected one version in _site/v, found ${versions}`);
const assets = join('_site/v', versions[0]);
const wasm = await import(pathToFileURL(join(assets, 'mobi2star.js')));
const exports = wasm.initSync({ module: readFileSync(join(assets, 'mobi2star_bg.wasm')) });

function check(condition, message) {
  if (!condition) throw new Error(message);
}

/** Every file of a zip archive, by name. Stored and deflated entries only. */
function unzip(zip) {
  const buf = Buffer.from(zip.buffer, zip.byteOffset, zip.byteLength);
  let end = buf.length - 22;
  while (end >= 0 && buf.readUInt32LE(end) !== 0x06054b50) end--;
  check(end >= 0, 'no end of central directory');
  const files = new Map();
  let at = buf.readUInt32LE(end + 16);
  for (let i = buf.readUInt16LE(end + 10); i > 0; i--) {
    check(buf.readUInt32LE(at) === 0x02014b50, 'bad central directory entry');
    const method = buf.readUInt16LE(at + 10);
    const packed = buf.readUInt32LE(at + 20);
    const size = buf.readUInt32LE(at + 24);
    const nameLength = buf.readUInt16LE(at + 28);
    const name = buf.toString('utf8', at + 46, at + 46 + nameLength);
    const local = buf.readUInt32LE(at + 42);
    const start = local + 30 + buf.readUInt16LE(local + 26) + buf.readUInt16LE(local + 28);
    const data = buf.subarray(start, start + packed);
    const bytes = method === 0 ? data : method === 8 ? inflateRawSync(data) : null;
    check(bytes && bytes.length === size, `${name}: unexpected method ${method} or size`);
    check(!files.has(name), `${name}: duplicate entry`);
    files.set(name, bytes);
    at += 46 + nameLength + buf.readUInt16LE(at + 30) + buf.readUInt16LE(at + 32);
  }
  return files;
}

/** Every file under `root`, by `/`-separated relative path. */
function tree(root) {
  const files = new Map();
  for (const entry of readdirSync(root, { recursive: true, withFileTypes: true })) {
    if (!entry.isFile()) continue;
    const path = join(entry.parentPath ?? entry.path, entry.name);
    files.set(relative(root, path).split('\\').join('/'), readFileSync(path));
  }
  return files;
}

/** Converts `book` in WebAssembly and with the CLI; both must agree. */
function compare(book, reader, labels) {
  const context = `${book} ${reader} ${labels}`;
  const stages = [];
  const started = performance.now();
  const converted = wasm.convert(
    readFileSync(book),
    JSON.stringify({ reader, labels }),
    (json) => stages.push(JSON.parse(json)),
  );
  const seconds = (performance.now() - started) / 1000;
  const summary = JSON.parse(converted.summary);
  const zip = converted.takeZip();
  converted.free();
  check(summary.zip_bytes === zip.length, `${context}: summary size`);
  check(summary.file_name === `${summary.folder} (StarDict).zip`, `${context}: file name`);
  check(stages[0].stage === 'parsing' && stages.at(-1).stage === 'checking', `${context}: stages`);
  const rendering = stages.filter((s) => s.stage === 'rendering');
  check(rendering.at(-1).done === rendering.at(-1).total, `${context}: progress total`);

  const actual = new Map();
  for (const [name, bytes] of unzip(zip)) {
    check(name.startsWith(`${summary.folder}/`), `${context}: ${name} is outside the folder`);
    actual.set(name.slice(summary.folder.length + 1), bytes);
  }
  const out = mkdtempSync(join(tmpdir(), 'mobi2star-smoke-'));
  try {
    execFileSync(cli, ['convert', book, '--output', join(out, 'o'), '--profile', 'stardict',
      '--reader', reader, '--labels', labels], { stdio: 'ignore' });
    const expected = tree(join(out, 'o/bundle/StarDict'));
    check(actual.size === expected.size, `${context}: ${actual.size} files, CLI wrote ${expected.size}`);
    for (const [name, bytes] of expected) {
      check(actual.get(name)?.equals(bytes), `${context}: ${name} differs from the CLI`);
    }
    const report = JSON.parse(readFileSync(join(out, 'o/bundle/report.json'), 'utf8'));
    check(isDeepStrictEqual(summary.report, report), `${context}: report differs from the CLI`);
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
  return { seconds, files: actual.size, summary };
}

function failsWith(code, source, choices) {
  try {
    wasm.convert(source, choices, () => {});
  } catch (error) {
    check(error.message.startsWith(`${code}: `), `expected ${code}, got ${error.message}`);
    return;
  }
  throw new Error(`expected ${code}, but the conversion succeeded`);
}

const choices = JSON.parse(wasm.choices());
check(wasm.version() === versions[0], 'version differs from the asset folder');
check(isDeepStrictEqual(choices.labels, ['en', 'zh']), 'label choices');
let n = 0;
for (const book of ['tests/fixtures/srcs.mobi', 'tests/fixtures/huff.mobi', 'tests/fixtures/uncompressed.mobi']) {
  for (const reader of choices.readers) {
    compare(book, reader, choices.labels[n++ % 2]);
  }
}
failsWith('MALFORMED', new TextEncoder().encode('not a mobi'), '{"reader":"koreader","labels":"en"}');
failsWith('OPTIONS', readFileSync('tests/fixtures/huff.mobi'), '{"reader":"kindle","labels":"en"}');
check(wasm.lastPanic() === undefined, 'no panic expected');
console.log(`WebAssembly output matches the CLI for ${n} conversions.`);

if (measured) {
  const { seconds, files, summary } = compare(measured, 'koreader', 'en');
  const mib = (bytes) => (bytes / 1024 / 1024).toFixed(0);
  console.log(`${summary.folder}: ${files} files, ${mib(summary.zip_bytes)} MiB zip, ` +
    `${seconds.toFixed(1)} s, WebAssembly memory ${mib(exports.memory.buffer.byteLength)} MiB`);
}
