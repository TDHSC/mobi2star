// Runs one conversion. The page starts a new worker for each conversion and
// terminates it afterwards, which frees all of its memory and is how Cancel
// works.
//
// In:  { file: File, choices: { reader, labels } }
// Out: { type: 'progress', stage }            stage as reported by Rust
//      { type: 'done', zip: Blob, fileName, summary }
//      { type: 'error', code, detail }
import init, { convert, lastPanic, maxInputBytes } from './mobi2star.js';

function fail(code, detail) {
  postMessage({ type: 'error', code, detail: String(detail ?? '') });
}

/** Splits the converter's `CODE: detail` error message. */
function conversionError(error) {
  if (error instanceof WebAssembly.RuntimeError) {
    let panic;
    try {
      panic = lastPanic();
    } catch {
      // The instance may be unusable after a trap.
    }
    return ['CRASH', panic ?? error.message];
  }
  const match = /^([A-Z]+): ([\s\S]*)$/.exec(error?.message ?? '');
  return match ? [match[1], match[2]] : ['OTHER', error?.message ?? error];
}

onmessage = async ({ data: { file, choices } }) => {
  try {
    await init();
  } catch (error) {
    return fail('LOAD', error?.message ?? error);
  }
  if (file.size > maxInputBytes()) {
    return fail('LIMIT', `${file.size} bytes; the browser limit is ${maxInputBytes()} bytes`);
  }
  let source;
  try {
    source = new Uint8Array(await file.arrayBuffer());
  } catch (error) {
    return fail('IO', error?.message ?? error);
  }
  try {
    const converted = convert(source, JSON.stringify(choices), (stage) => {
      postMessage({ type: 'progress', stage: JSON.parse(stage) });
    });
    source = null; // Let the page's copy go before the archive is copied out.
    const zip = new Blob([converted.takeZip()], { type: 'application/zip' });
    const message = { type: 'done', zip, fileName: converted.fileName, summary: JSON.parse(converted.summary) };
    converted.free();
    postMessage(message);
  } catch (error) {
    fail(...conversionError(error));
  }
};
