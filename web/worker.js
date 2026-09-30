// Runs one conversion. The page starts a new worker for each conversion and
// terminates it afterwards, which frees all of its memory and is how Cancel
// works.
//
// In:  { file: File, choices: { reader, labels } }
// Out: { type: 'progress', stage }            stage as reported by Rust
//      { type: 'done', zip: Blob, fileName, summary }
//      { type: 'error', code, detail }
import init, { Source, convert, lastPanic } from './mobi2star.js';

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
  // The file goes into WebAssembly memory chunk by chunk, so the worker
  // never holds a second whole copy of it. `Source` refuses files over the
  // browser limit before anything is read.
  let source;
  try {
    source = new Source(file.size);
    const reader = file.stream().getReader();
    for (let chunk = await reader.read(); !chunk.done; chunk = await reader.read()) {
      source.push(chunk.value);
    }
  } catch (error) {
    const [code, detail] = conversionError(error);
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
    fail(...conversionError(error));
  }
};
