// Runs one conversion. The page starts a new worker for each conversion and
// terminates it afterwards, which frees all of its memory and is how Cancel
// works.
//
// In:  { file: File, choices: { reader, labels } }
// Out: { type: 'progress', stage }            stage as reported by Rust
//      { type: 'done', zip: Blob, fileName, summary }
//      { type: 'error', code, detail }
import init, { Source, convert, lastPanic } from './mobi2star.js';
import { describe, fill } from './worker-common.js';

function fail(code, detail) {
  postMessage({ type: 'error', code, detail: String(detail ?? '') });
}

onmessage = async ({ data: { file, choices } }) => {
  try {
    await init();
  } catch (error) {
    return fail('LOAD', error?.message ?? error);
  }
  // The file goes into WebAssembly memory chunk by chunk, so the worker
  // never holds a second whole copy of it. `Source` refuses files over the
  // browser limit before anything is read.
  let source;
  try {
    source = await fill(new Source(file.size), file);
  } catch (error) {
    const [code, detail] = describe(error, lastPanic);
    return fail(code === 'OTHER' ? 'IO' : code, detail);
  }
  try {
    const converted = convert(source, JSON.stringify(choices), (stage) => {
      postMessage({ type: 'progress', stage: JSON.parse(stage) });
    });
    const zip = new Blob([converted.takeZip()], { type: 'application/zip' });
    const message = { type: 'done', zip, fileName: converted.fileName, summary: JSON.parse(converted.summary) };
    converted.free();
    postMessage(message);
  } catch (error) {
    fail(...describe(error, lastPanic));
  }
};
