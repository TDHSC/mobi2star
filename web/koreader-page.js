// Draws KOReader's dictionary popup with MuPDF, the engine KOReader uses
// for it: the document KOReader builds, laid out in the popup's text box
// at its font size and cut into box-sized pages, one drawn at a time.
// The preview worker and tools/web-smoke.mjs share it.

/**
 * @param mupdf  the mupdf.js module
 * @param rules  KOReader's font rules: the WebAssembly `Preview` class
 *               (`koreaderFont`, `koreaderLastFont`, `koreaderWithLastFont`)
 * @param load   font file => its bytes. It must answer synchronously:
 *               MuPDF asks in the middle of a layout, and remembers a
 *               fallback it was refused for as long as it runs.
 */
export class KoreaderPages {
  constructor(mupdf, rules, load) {
    this.mupdf = mupdf;
    this.rules = rules;
    this.load = load;
    this.fonts = new Map();
    /** Font files that could not be loaded; MuPDF drew with its own. */
    this.missing = new Set();
    this.open = null;
    mupdf.installLoadFontFunction((family, script, bold, italic) =>
      this.font(rules.koreaderFont(String(family), String(script), !!bold, !!italic)),
    );
  }

  /** The loaded font `file`, or null. */
  font(file) {
    if (!file || this.missing.has(file)) return null;
    if (!this.fonts.has(file)) {
      try {
        this.fonts.set(file, new this.mupdf.Font(file, this.load(file)));
      } catch {
        // An exception must not unwind through MuPDF.
        this.missing.add(file);
        return null;
      }
    }
    return this.fonts.get(file);
  }

  /** `html` opened and laid out in `geometry`. */
  layout(html, { width, height, em }) {
    const document = this.mupdf.Document.openDocument(new TextEncoder().encode(html), 'text/html');
    try {
      document.layout(width, height, em);
    } catch (error) {
      document.destroy();
      throw error;
    }
    return document;
  }

  /**
   * The characters MuPDF drew as boxes, having no font for them, that
   * KOReader's last font has.
   */
  boxes(document) {
    const found = new Set();
    for (let n = 0, pages = document.countPages(); n < pages; n++) {
      const page = document.loadPage(n);
      const text = page.toStructuredText();
      text.walk({
        onChar(char, origin, font) {
          if (font.encodeCharacter(char.codePointAt(0)) === 0) found.add(char);
        },
      });
      text.destroy();
      page.destroy();
    }
    const last = found.size ? this.font(this.rules.koreaderLastFont()) : null;
    return last ? [...found].filter((char) => last.encodeCharacter(char.codePointAt(0)) > 0) : [];
  }

  /**
   * The document for `html` laid out in `geometry`, reusing the last one.
   * Characters that only KOReader's last font has are set in it, and the
   * page laid out again.
   */
  document(html, geometry) {
    const { width, height, em } = geometry;
    const open = this.open;
    if (open?.html === html && open.width === width && open.height === height && open.em === em) {
      return open.document;
    }
    this.close();
    let document = this.layout(html, geometry);
    const boxes = this.boxes(document);
    if (boxes.length) {
      document.destroy();
      document = this.layout(this.rules.koreaderWithLastFont(html, boxes.join('')), geometry);
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
