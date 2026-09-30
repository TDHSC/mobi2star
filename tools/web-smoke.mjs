// Smoke test for the browser build, with Node and no npm packages. It loads
// _site/ as the page's worker does and requires every archive to unzip to
// exactly the files `mobi2star convert --profile stardict` writes. It also
// checks that the page's text covers every choice in both languages.
//
//   node tools/web-smoke.mjs CLI [--measure BOOK.mobi]
//
// CLI is a built mobi2star binary. --measure also converts BOOK, prints the
// time and WebAssembly memory, and compares it the same way.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
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

/** A `Source` filled in 1 MiB chunks, as the worker fills it from a File. */
function sourceOf(bytes) {
  const source = new wasm.Source(bytes.length);
  for (let at = 0; at < bytes.length; at += 1 << 20) source.push(bytes.subarray(at, at + (1 << 20)));
  return source;
}

/** Converts `book` in WebAssembly and with the CLI; both must agree. */
function compare(book, reader, labels) {
  const context = `${book} ${reader} ${labels}`;
  const stages = [];
  const started = performance.now();
  const converted = wasm.convert(
    sourceOf(readFileSync(book)),
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

/** `run` must throw the converter's error with `code`. */
function failsWith(code, run) {
  try {
    run();
  } catch (error) {
    check(error.message.startsWith(`${code}: `), `expected ${code}, got ${error.message}`);
    return;
  }
  throw new Error(`expected ${code}, but the conversion succeeded`);
}

const choices = JSON.parse(wasm.choices());
check(wasm.version() === versions[0], 'version differs from the asset folder');
check(isDeepStrictEqual(choices.labels, ['en', 'zh']), 'label choices');

/** Every key path in a nested object, with arrays counted by length. */
function shape(value, prefix = '') {
  if (Array.isArray(value)) return [`${prefix}[${value.length}]`];
  if (value === null || typeof value !== 'object') return [prefix];
  return Object.keys(value).sort().flatMap((key) => shape(value[key], `${prefix}.${key}`));
}
const { LANGUAGES, TEXT, format } = await import(pathToFileURL(join(assets, 'i18n.js')));
check(isDeepStrictEqual(LANGUAGES, Object.keys(TEXT)), 'languages');
const fidelities = [...new Set(Object.values(choices.apps).map((facts) => facts.fidelity))].sort();
for (const lang of LANGUAGES) {
  check(isDeepStrictEqual(shape(TEXT[lang]), shape(TEXT.en)), `${lang} text has other keys than en`);
  check(isDeepStrictEqual(Object.keys(TEXT[lang].readers), choices.readers), `${lang} readers`);
  check(isDeepStrictEqual(Object.keys(TEXT[lang].labels), choices.labels), `${lang} labels`);
  const sorted = (object) => Object.keys(object).sort();
  check(isDeepStrictEqual(sorted(TEXT[lang].apps), sorted(choices.apps)), `${lang} apps`);
  check(isDeepStrictEqual(Object.keys(TEXT[lang].fidelity).sort(), fidelities), `${lang} fidelity`);
  check(isDeepStrictEqual(Object.keys(TEXT[lang].stays), choices.stays), `${lang} stays`);
  const screens = choices.koreader.screens.map((screen) => screen.id);
  check(isDeepStrictEqual(Object.keys(TEXT[lang].screens), screens), `${lang} screens`);
}
check(isDeepStrictEqual(Object.keys(choices.previews).sort(), [...choices.readers].sort()), 'every choice has previews');
check(format('{a} of {b}', { a: 1, b: 2 }) === '1 of 2', 'format');
for (const name of readFileSync(join(assets, 'files.txt'), 'utf8').trim().split('\n')) {
  check(/^[0-9A-Za-z._-]+$/.test(name) && !name.startsWith('.'), `asset name ${name} would not be kept`);
}
const page = readFileSync('_site/index.html', 'utf8');
check(!page.includes('__VERSION__') && page.includes(`v/${versions[0]}/app.js`), 'index.html version');
let n = 0;
for (const book of ['tests/fixtures/srcs.mobi', 'tests/fixtures/huff.mobi', 'tests/fixtures/uncompressed.mobi']) {
  for (const reader of choices.readers) {
    compare(book, reader, choices.labels[n++ % 2]);
  }
}
const huff = readFileSync('tests/fixtures/huff.mobi');
const convertWith = (bytes, choices) => () => wasm.convert(sourceOf(bytes), choices, () => {});
// The preview opens a converted zip the way the preview worker does.
const { fill } = await import(pathToFileURL(join(assets, 'worker-common.js')));
async function previewOf(book, reader) {
  const converted = wasm.convert(sourceOf(readFileSync(book)), JSON.stringify({ reader, labels: 'en' }), () => {});
  const zip = new Blob([converted.takeZip()]);
  converted.free();
  return new wasm.Preview(await fill(wasm.Source.forArchive(zip.size), zip), reader);
}
{
  const preview = await previewOf('tests/fixtures/srcs.mobi', 'universal');
  const info = JSON.parse(preview.info());
  check(isDeepStrictEqual(info.apps, choices.previews.universal), 'preview apps');
  const found = JSON.parse(preview.search('goldendict-ng', 'run'));
  check(found.outcome === 'view' && found.results.length === 2, 'preview search');
  const link = /href="(bword:\/\/[^"]+)"/.exec(found.documents[0])[1];
  const followed = JSON.parse(preview.follow('goldendict-ng', link, found.results[0].entry));
  check(followed.outcome === 'view' && followed.scroll_to, 'preview follows and scrolls');
  check(JSON.parse(preview.follow('readest', link, 0)).reason === 'not-followed', 'readest stays');
  check(JSON.parse(preview.suggest('ru', 12)).includes('run'), 'preview suggestions');
  preview.free();
}
// A KOReader page, drawn as the preview worker draws it but with the fonts
// read from disk.
{
  const mupdf = await import(pathToFileURL(join(assets, 'mupdf.js')));
  const { KoreaderPages } = await import(pathToFileURL(join(assets, 'koreader-page.js')));
  const fontFiles = [
    ...JSON.parse(wasm.Preview.koreaderFonts()),
    wasm.Preview.koreaderFont('undefined', 'TC', false, false),
    wasm.Preview.koreaderFont('undefined', 'Hebrew', false, false),
  ];
  for (const file of fontFiles) check(existsSync(join(assets, file)), `${file} is bundled`);
  const pages = new KoreaderPages(mupdf, wasm.Preview, (file) => readFileSync(join(assets, file)));
  const preview = await previewOf('tests/fixtures/srcs.mobi', 'koreader');
  const found = JSON.parse(preview.search('koreader', 'run'));
  const { screens, fontSizes } = choices.koreader;
  const geometry = JSON.parse(wasm.Preview.koreaderGeometry(screens[0].id, fontSizes.default));
  const drawn = pages.draw(found.documents[0], geometry, 0);
  check(drawn.width === geometry.width && drawn.height === geometry.height, 'KOReader page size');
  check(drawn.pixels.length === drawn.width * drawn.height * 4 && drawn.pixels.some((v) => v < 128), 'KOReader page drawn');
  check(drawn.text.startsWith('run\n') && drawn.text.includes('(query : run)'), 'KOReader page text');
  check(pages.fonts.has('NotoSans-Regular.ttf') && pages.fonts.has('NotoSansCJKsc-Regular.otf'), 'KOReader fonts');
  check(pages.missing.size === 0, 'no font is missing');
  const hrefs = [...found.documents[0].matchAll(/href="(bword:[^"]+)"/g)].map((match) => match[1]);
  const uris = drawn.links.map((link) => link.uri);
  check(drawn.pages === 1 && isDeepStrictEqual(uris, hrefs), 'KOReader page links');
  check(drawn.links.every((link) => link.label), 'KOReader links have text');
  const followed = JSON.parse(preview.follow('koreader', uris[0], found.results[0].entry));
  check(followed.outcome === 'view' && followed.results[0].headword === 'café', 'KOReader follows a link');
  const small = pages.draw(found.documents[0], JSON.parse(wasm.Preview.koreaderGeometry('phone', fontSizes.max)), 9);
  check(small.page === small.pages - 1, 'KOReader clamps the page');
  // U+25B8 is in neither Noto Sans nor Noto Sans CJK SC: KOReader draws it
  // with FreeSerif.
  const symbol = found.documents[0].replace('<body>', '<body><p>\u25b8 bunch up</p>');
  pages.draw(symbol, geometry, 0);
  check(pages.fonts.has(wasm.Preview.koreaderLastFont()), 'the last font draws what no other font has');
  const drawnWith = new Map();
  pages.open.document.loadPage(0).toStructuredText().walk({
    onChar: (char, origin, font) => drawnWith.set(char, font.getName()),
  });
  check(drawnWith.get('\u25b8') === 'FreeSerif.ttf' && drawnWith.get('b') === 'NotoSans-Regular.ttf', 'U+25B8 in FreeSerif');
  pages.close();
  preview.free();
}
failsWith('MALFORMED', convertWith(new TextEncoder().encode('not a mobi'), '{"reader":"koreader","labels":"en"}'));
failsWith('OPTIONS', convertWith(huff, '{"reader":"kindle","labels":"en"}'));
failsWith('LIMIT', () => new wasm.Source(256 * 1024 * 1024 + 1));
failsWith('IO', () => new wasm.Source(2).push(huff.subarray(0, 3)));
failsWith('IO', () => wasm.convert(new wasm.Source(1), '{"reader":"koreader","labels":"en"}', () => {}));
check(wasm.lastPanic() === undefined, 'no panic expected');
console.log(`WebAssembly output matches the CLI for ${n} conversions.`);

if (measured) {
  const { seconds, files, summary } = compare(measured, 'koreader', 'en');
  const mib = (bytes) => (bytes / 1024 / 1024).toFixed(0);
  console.log(`${summary.folder}: ${files} files, ${mib(summary.zip_bytes)} MiB zip, ` +
    `${seconds.toFixed(1)} s, WebAssembly memory ${mib(exports.memory.buffer.byteLength)} MiB`);
}
