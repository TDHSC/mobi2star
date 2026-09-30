// Shared by the conversion and preview workers.

/**
 * Fills a WebAssembly `Source` from a File or Blob chunk by chunk, so the
 * worker never holds a second whole copy of it.
 */
export async function fill(source, blob) {
  const reader = blob.stream().getReader();
  for (let chunk = await reader.read(); !chunk.done; chunk = await reader.read()) {
    source.push(chunk.value);
  }
  return source;
}

/**
 * The code and detail of an error thrown by the WebAssembly: its
 * `CODE: detail` message, or CRASH for a trap, with the panic message when
 * there was one (running out of memory leaves none).
 */
export function describe(error, lastPanic) {
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
  return match ? [match[1], match[2]] : ['OTHER', String(error?.message ?? error)];
}
