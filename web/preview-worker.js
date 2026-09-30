// Holds one converted dictionary for the preview and answers lookups the
// way the chosen reader would. The page starts it when Preview is first
// opened and terminates it when the conversion it belongs to is gone.
//
// In:  { id, type: 'open', zip: Blob, reader }
//      { id, type: 'search', app, word }
//      { id, type: 'follow', app, href, current }
//      { id, type: 'suggest', prefix }
//      { id, type: 'random' }
//      { id, type: 'draw', query, entry, n, screen, fontSize, page }
//        (KOReader: page `page` of result `n`, entry `entry`, of `query`)
// Out: { id, ok: true, value } or { id, ok: false, code, detail }
import init, { Preview, Source, choices, lastPanic } from './mobi2star.js';
import { describe, fill } from './worker-common.js';
import { KoreaderPages } from './koreader-page.js';

let preview = null;
let koreader = null;
/** The KOReader document last composed, so turning pages reuses it. */
let composed = { key: null, html: null };

const asset = (file) => new URL(`./${file}`, import.meta.url);

/**
 * MuPDF and KOReader's fonts, loaded on the first KOReader page. Noto
 * Sans is fetched up front; a fallback font is fetched when MuPDF asks for
 * it, synchronously, which only a worker may do.
 */
function koreaderPages() {
  koreader ??= (async () => {
    const [mupdf, ...fonts] = await Promise.all([
      import('./mupdf.js'),
      ...JSON.parse(Preview.koreaderFonts()).map(async (file) => {
        const response = await fetch(asset(file));
        if (!response.ok) throw new Error(`LOAD: ${file}: ${response.status}`);
        return [file, new Uint8Array(await response.arrayBuffer())];
      }),
    ]);
    const fetched = new Map(fonts);
    return new KoreaderPages(mupdf, Preview, (file) => {
      const bytes = fetched.get(file);
      fetched.delete(file);
      return bytes ?? fetchNow(file);
    });
  })().catch((error) => {
    koreader = null;
    throw error;
  });
  return koreader;
}

function fetchNow(file) {
  const request = new XMLHttpRequest();
  request.open('GET', asset(file), false);
  request.responseType = 'arraybuffer';
  request.send();
  if (request.status !== 200) throw new Error(`${file}: ${request.status}`);
  return new Uint8Array(request.response);
}

const handlers = {
  async open({ zip, reader }) {
    await init();
    preview = new Preview(await fill(Source.forArchive(zip.size), zip), reader);
    return { info: JSON.parse(preview.info()), choices: JSON.parse(choices()) };
  },
  search: ({ app, word }) => JSON.parse(preview.search(app, word)),
  follow: ({ app, href, current }) => JSON.parse(preview.follow(app, href, current)),
  suggest: ({ prefix }) => JSON.parse(preview.suggest(prefix, 12)),
  random: () => preview.headwordAt(Math.random()) ?? null,
  async draw({ query, entry, n, screen, fontSize, page }) {
    const pages = await koreaderPages();
    const key = JSON.stringify([entry, n === 0 ? query : null]);
    if (composed.key !== key) composed = { key, html: preview.koreaderDocument(query, entry, n) };
    const { html } = composed;
    const geometry = JSON.parse(Preview.koreaderGeometry(screen, fontSize) ?? 'null');
    if (!geometry) throw new Error(`OPTIONS: unknown screen ${screen}`);
    return { ...pages.draw(html, geometry, page), missing: [...pages.missing] };
  },
};

onmessage = async ({ data }) => {
  try {
    const value = await handlers[data.type](data);
    postMessage({ id: data.id, ok: true, value }, value?.pixels ? [value.pixels.buffer] : []);
  } catch (error) {
    const [code, detail] = describe(error, lastPanic);
    postMessage({ id: data.id, ok: false, code, detail });
  }
};
