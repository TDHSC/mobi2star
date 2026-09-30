// Draws KOReader's dictionary popup with MuPDF, the engine KOReader uses
// for it: the document KOReader builds, laid out in the popup's text box
// at its font size and cut into box-sized pages, one drawn at a time.
// The preview worker and tools/web-smoke.mjs share it.

/**
 * @param mupdf    the mupdf.js module
 * @param fontFor  (family, script, bold, italic) => the font file KOReader
 *                 would use, or undefined for MuPDF's own
 * @param load     font file => its bytes. It must answer synchronously:
 *                 MuPDF asks in the middle of a layout, and remembers a
 *                 fallback it was refused for as long as it runs.
 */
export class KoreaderPages {
  constructor(mupdf, fontFor, load) {
    this.mupdf = mupdf;
    this.fonts = new Map();
    /** Font files that could not be loaded; MuPDF drew with its own. */
    this.missing = new Set();
    this.open = null;
    mupdf.installLoadFontFunction((family, script, bold, italic) => {
      const file = fontFor(String(family), String(script), !!bold, !!italic);
      if (!file || this.missing.has(file)) return null;
      if (!this.fonts.has(file)) {
        try {
          this.fonts.set(file, new mupdf.Font(file, load(file)));
        } catch {
          // An exception must not unwind through MuPDF.
          this.missing.add(file);
          return null;
        }
      }
      return this.fonts.get(file);
    });
  }

  /** The document for `html` laid out in `geometry`, reusing the last one. */
  document(html, { width, height, em }) {
    const open = this.open;
    if (open?.html === html && open.width === width && open.height === height && open.em === em) {
      return open.document;
    }
    this.close();
    const document = this.mupdf.Document.openDocument(new TextEncoder().encode(html), 'text/html');
    try {
      document.layout(width, height, em);
    } catch (error) {
      document.destroy();
      throw error;
    }
    this.open = { html, width, height, em, document };
    return document;
  }

  /**
   * Page `number` (clamped) of `html`: its grayscale image as RGBA pixels,
   * its links with their rectangles and text, and its text.
   */
  draw(html, geometry, number) {
    const { Matrix, ColorSpace } = this.mupdf;
    const document = this.document(html, geometry);
    const pages = document.countPages();
    const page = Math.min(Math.max(0, number), pages - 1);
    const loaded = document.loadPage(page);
    const text = loaded.toStructuredText();
    const pixmap = loaded.toPixmap(Matrix.identity, ColorSpace.DeviceGray, false);
    try {
      const width = pixmap.getWidth();
      const height = pixmap.getHeight();
      const gray = pixmap.getPixels();
      const stride = pixmap.getStride();
      const pixels = new Uint8ClampedArray(width * height * 4);
      for (let y = 0, out = 0; y < height; y++) {
        for (let x = y * stride, end = x + width; x < end; x++, out += 4) {
          pixels[out] = pixels[out + 1] = pixels[out + 2] = gray[x];
          pixels[out + 3] = 255;
        }
      }
      const links = loaded.getLinks().map((link) => {
        const rect = link.getBounds();
        const middle = (rect[1] + rect[3]) / 2;
        const label = text.copy([rect[0] + 1, middle], [rect[2] - 1, middle]).trim();
        const uri = link.getURI();
        link.destroy();
        return { rect, uri, label };
      });
      return { page, pages, width, height, pixels, links, text: text.asText() };
    } finally {
      pixmap.destroy();
      text.destroy();
      loaded.destroy();
    }
  }

  close() {
    this.open?.document.destroy();
    this.open = null;
  }
}
