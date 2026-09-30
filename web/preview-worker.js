// Holds one converted dictionary for the preview and answers lookups the
// way the chosen reader would. The page starts it when Preview is first
// opened and terminates it when the conversion it belongs to is gone.
//
// In:  { id, type: 'open', zip: Blob, reader }
//      { id, type: 'search', app, word }
//      { id, type: 'follow', app, href, current }
//      { id, type: 'suggest', prefix }
//      { id, type: 'random' }
// Out: { id, ok: true, value } or { id, ok: false, code, detail }
import init, { Preview, Source, choices, lastPanic } from './mobi2star.js';
import { describe, fill } from './worker-common.js';

let preview = null;

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
};

onmessage = async ({ data }) => {
  try {
    postMessage({ id: data.id, ok: true, value: await handlers[data.type](data) });
  } catch (error) {
    const [code, detail] = describe(error, lastPanic);
    postMessage({ id: data.id, ok: false, code, detail });
  }
};
