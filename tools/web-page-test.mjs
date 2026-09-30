// Drives the built page in headless Chrome: converts a fixture as a visitor
// would, opens the preview and checks what only a browser can: the preview
// panel's behaviour, its frame's link handling and MuPDF's canvas.
//
//   ./tools/build-web.sh
//   node tools/web-page-test.mjs
//
// Chrome is taken from $CHROME, else found on the PATH or in its usual
// macOS place. It is controlled over --remote-debugging-pipe, so the test
// needs only Node. The site is served from _site/ by this script.
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join, resolve, sep } from 'node:path';

function check(condition, message) {
  if (!condition) throw new Error(message);
}

function findChrome() {
  const candidates = [
    process.env.CHROME,
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    ...['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser'].flatMap((name) =>
      (process.env.PATH ?? '').split(':').map((dir) => join(dir, name)),
    ),
  ];
  const found = candidates.find((path) => path && existsSync(path));
  check(found, 'Chrome not found: set CHROME to its executable.');
  return found;
}

/** Serves _site/ on a free local port. */
async function serve(root) {
  const types = {
    '.html': 'text/html; charset=utf-8',
    '.js': 'text/javascript',
    '.css': 'text/css',
    '.wasm': 'application/wasm',
    '.svg': 'image/svg+xml',
    '.ttf': 'font/ttf',
    '.otf': 'font/otf',
    '.txt': 'text/plain',
  };
  const server = createServer((request, response) => {
    const path = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
    let file = resolve(root, `.${path}`);
    if (file !== root && !file.startsWith(root + sep)) file = root;
    if (existsSync(file) && statSync(file).isDirectory()) file = join(file, 'index.html');
    if (!existsSync(file)) {
      response.writeHead(404).end();
      return;
    }
    response.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' });
    response.end(readFileSync(file));
  });
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  return server;
}

/** Chrome DevTools Protocol over the pipe: JSON messages ended by NUL. */
function launch(chrome, profile) {
  const args = [
    '--headless=new',
    '--remote-debugging-pipe',
    `--user-data-dir=${profile}`,
    '--no-first-run',
    '--no-default-browser-check',
    // CI runners cannot always give Chrome its sandbox; the test loads
    // nothing but the local site.
    ...(process.env.CI ? ['--no-sandbox'] : []),
    'about:blank',
  ];
  const child = spawn(chrome, args, { stdio: ['ignore', 'ignore', 'pipe', 'pipe', 'pipe'] });
  let log = '';
  child.stderr.on('data', (chunk) => (log = (log + chunk).slice(-4000)));
  const pending = new Map();
  const listeners = [];
  let buffer = '';
  let next = 1;
  child.stdio[4].setEncoding('utf8');
  child.stdio[4].on('data', (chunk) => {
    buffer += chunk;
    for (let end = buffer.indexOf('\0'); end >= 0; end = buffer.indexOf('\0')) {
      const message = JSON.parse(buffer.slice(0, end));
      buffer = buffer.slice(end + 1);
      if (message.id && pending.has(message.id)) {
        const { resolve: done, reject, method } = pending.get(message.id);
        pending.delete(message.id);
        if (message.error) reject(new Error(`${method}: ${message.error.message}`));
        else done(message.result);
      } else {
        for (const listener of listeners) listener(message);
      }
    }
  });
  const send = (method, params = {}, sessionId) =>
    new Promise((done, reject) => {
      const id = next++;
      pending.set(id, { resolve: done, reject, method });
      child.stdio[3].write(`${JSON.stringify({ id, method, params, sessionId })}\0`);
    });
  return { child, send, listeners, log: () => log };
}

const site = resolve('_site');
check(existsSync(join(site, 'index.html')), 'Build the site first: ./tools/build-web.sh');
const server = await serve(site);
const profile = mkdtempSync(join(tmpdir(), 'mobi2star-page-'));
const browser = launch(findChrome(), profile);
let failed = true;
try {
  const { targetId } = await browser.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await browser.send('Target.attachToTarget', { targetId, flatten: true });
  const page = (method, params) => browser.send(method, params, sessionId);
  const exceptions = [];
  browser.listeners.push((message) => {
    if (message.sessionId === sessionId && message.method === 'Runtime.exceptionThrown') {
      const details = message.params.exceptionDetails;
      exceptions.push(details.exception?.description ?? details.text);
    }
  });
  await page('Runtime.enable');
  await page('Page.enable');

  const run = async (expression) => {
    const result = await page('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
    }
    return result.result.value;
  };
  const until = async (expression, what, ms = 60000) => {
    for (const end = Date.now() + ms; Date.now() < end; await new Promise((done) => setTimeout(done, 100))) {
      if (await run(expression)) return;
    }
    throw new Error(`timed out waiting for ${what}`);
  };

  // The page in English, whatever the system's language.
  const url = `http://127.0.0.1:${server.address().port}/`;
  await page('Page.navigate', { url });
  await until(`document.readyState === 'complete' && !!document.getElementById('file')`, 'the page');
  await run(`localStorage.setItem('mobi2star.language', 'en')`);
  await page('Page.navigate', { url });
  await until(`document.readyState === 'complete' && document.documentElement.lang === 'en'`, 'the page in English');
  const { root } = await page('DOM.getDocument');
  const { nodeId } = await page('DOM.querySelector', { nodeId: root.nodeId, selector: '#file' });
  await page('DOM.setFileInputFiles', { nodeId, files: [resolve('tests/fixtures/srcs.mobi')] });
  await run(`(() => {
    const reader = document.getElementById('reader');
    reader.value = 'universal';
    reader.dispatchEvent(new Event('change'));
    document.querySelector('form button[type=submit]').click();
  })()`);
  await until(`!document.getElementById('preview-toggle').closest('[hidden]')`, 'the conversion');
  await run(`document.getElementById('preview-toggle').click()`);

  // Helpers inside the page. `settled`: the current view is on screen.
  await run(`window.t = {
    p: document.getElementById('preview'),
    q: (selector) => t.p.querySelector(selector),
    status: () => t.q('[role=status]').textContent,
    pageText: () => t.q('.preview-page .visually-hidden').textContent,
    app(name) {
      const select = t.q('#preview-app');
      select.value = name;
      select.dispatchEvent(new Event('change'));
    },
    search(word) {
      t.q('#preview-word').value = word;
      t.q('#preview-word').form.requestSubmit();
    },
    settled() {
      const status = t.status();
      if (!status || status.endsWith('…')) return false;
      if (t.q('.preview-stage').hidden) return true;
      return !t.q('.preview-page').hidden || !t.q('iframe').hidden;
    },
  }`);
  const settle = (what) => until(`t.settled()`, what);
  await settle('the first random word');

  // KOReader: results one at a time, drawn by MuPDF, with links on them.
  await run(`t.search('run')`);
  await until(`t.pageText().includes('(query : run)')`, 'KOReader drawing "run"');
  check(await run(`t.status()`) === '2 results for “run”.', 'KOReader status');
  check(await run(`t.q('canvas').width > 0 && t.p.querySelectorAll('.preview-link').length > 0`), 'KOReader page and links');
  check((await run(`t.q('.preview-pager').textContent`)).includes('Result 1 of 2'), 'result stepper');
  // While a new lookup draws, the previous page is gone, links included.
  await run(`t.seen = [];
    t.observer = new MutationObserver(() => t.seen.push([t.status(), t.pageText(), t.p.querySelectorAll('.preview-link').length]));
    t.observer.observe(t.q('[role=status]'), { childList: true, characterData: true, subtree: true });
    t.search('café')`);
  await until(`t.pageText().includes('(query : café)')`, 'KOReader drawing "café"');
  const seen = await run(`t.observer.disconnect(), t.seen`);
  const switched = seen.find(([status]) => status.includes('café'));
  check(switched && switched[1] === '' && switched[2] === 0, `stale page while drawing: ${JSON.stringify(seen)}`);
  await run(`t.search('run')`);
  await until(`t.pageText().includes('(query : run)')`, 'KOReader drawing "run" again');
  await run(`t.q('.preview-pager button[aria-label="Next result"]').click()`);
  await until(`t.q('.preview-pager').textContent.includes('Result 2 of 2') && !t.q('.preview-page').hidden`, 'result 2');
  check(!(await run(`t.pageText()`)).includes('(query'), 'only the first result carries the query');
  await run(`t.q('.preview-pager button[aria-label="Previous result"]').click()`);
  await until(`t.q('.preview-pager').textContent.includes('Result 1 of 2') && !t.q('.preview-page').hidden`, 'result 1');
  await run(`t.q('.preview-link').click()`);
  await until(`t.status() === 'Showing “café”.' && !t.q('.preview-page').hidden`, 'the followed link');
  check(await run(`t.q('#preview-word').value`) === 'café', 'a followed link shows the headword, not its key');

  // An emptied font size keeps the size.
  check(
    await run(`(() => { const f = t.q('#preview-font-size'); f.value = ''; f.dispatchEvent(new Event('change')); return f.value; })()`) === '20',
    'an empty font size is ignored',
  );

  // A lookup started while Random waits wins.
  await run(`t.q('.preview-search button.quiet').click(); t.search('run')`);
  await settle('the lookup after Random');
  await until(`t.pageText().includes('(query : run)')`, 'the typed word to stay');
  await new Promise((done) => setTimeout(done, 500));
  check(await run(`t.q('#preview-word').value`) === 'run', 'Random overrode a typed lookup');

  // Switching readers: GoldenDict-ng folds "cafe" to "café"; KOReader then
  // finds nothing and shows nothing of GoldenDict-ng.
  await run(`t.app('goldendict-ng')`);
  await settle('GoldenDict-ng');
  await run(`t.search('cafe')`);
  await until(`t.status() === 'Showing “café”.'`, 'GoldenDict-ng folding');
  await run(`t.app('koreader')`);
  await until(`t.status() === 'This reader finds nothing for “cafe”.'`, 'KOReader missing "cafe"');
  check(await run(`t.q('.preview-stage').hidden`), 'nothing of the previous reader stays on screen');

  // The frame: every kind of link is handed to the reader's rules, and the
  // frame never navigates.
  await run(`t.app('goldendict-ng')`);
  await run(`t.search('run')`);
  await until(`t.status() === '2 results for “run”.' && !t.q('iframe').hidden`, 'GoldenDict-ng "run"');
  const before = await run(`history.length`);
  const external = 'A web link: the reader would open a browser, which the preview does not.';
  for (const [id, markup] of [
    ['plain', '<a id="plain" href="https://example.invalid/a">a</a>'],
    ['area', '<map name="m"><area id="area" shape="rect" coords="0,0,9,9" href="https://example.invalid/area"></map>'],
    ['svg', '<svg><a id="svg" xlink:href="https://example.invalid/svg"><text y="9">s</text></a></svg>'],
    ['math', '<math><mi id="math" href="https://example.invalid/math">x</mi></math>'],
  ]) {
    await run(`(() => {
      const doc = t.q('iframe').contentDocument;
      doc.body.insertAdjacentHTML('beforeend', ${JSON.stringify(markup)});
      const link = doc.getElementById(${JSON.stringify(id)});
      (link.querySelector('text') ?? link).dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    })()`);
    await until(`t.status() === ${JSON.stringify(external)}`, `the ${id} link to be handled`);
    check(
      await run(`t.q('iframe').contentWindow.location.pathname.endsWith('/frame.html') && history.length === ${before}`),
      `the ${id} link navigated`,
    );
    await run(`t.search('run')`);
    await until(`t.status() === '2 results for “run”.'`, 'GoldenDict-ng "run" once more');
  }

  // The preview's own error texts.
  const failure = await run(`(async () => {
    const version = document.getElementById('version').dataset.version;
    const { PreviewPanel } = await import('./v/' + version + '/preview.js');
    const { TEXT } = await import('./v/' + version + '/i18n.js');
    const container = document.createElement('section');
    const panel = new PreviewPanel({ container, text: () => TEXT.en, zip: new Blob(['not a zip']), reader: 'koreader' });
    const line = () => container.querySelector('[role=status]').textContent;
    for (let n = 0; n < 100 && !line().startsWith(TEXT.en.ui.previewFailed); n++) await new Promise((done) => setTimeout(done, 100));
    const shown = line();
    panel.close();
    return [shown, TEXT.en.ui.previewFailed + ' ' + TEXT.en.previewErrors.OTHER];
  })()`);
  check(failure[0].startsWith(failure[1]), `preview error text: ${failure[0]}`);

  check(exceptions.length === 0, `uncaught exceptions: ${exceptions.join('\n')}`);
  failed = false;
  console.log('The page converts, previews, draws with MuPDF and keeps its frame in place.');
} finally {
  if (failed) console.error(browser.log());
  const exited = new Promise((done) => browser.child.once('exit', done));
  if (browser.child.exitCode === null) {
    browser.child.kill();
    await exited;
  }
  server.close();
  rmSync(profile, { recursive: true, force: true, maxRetries: 5 });
}
