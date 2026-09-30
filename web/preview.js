// The preview panel: search the converted dictionary and follow its links,
// drawn as the chosen reader draws them. Lookups, link rules and, for
// KOReader, MuPDF run in the preview worker; this file only draws what
// comes back.
import { format } from './i18n.js';

/** Frame sizes in CSS pixels: a desktop window, a popup, or a phone. */
const FRAME_SIZES = {
  'goldendict-ng': { width: '100%', height: '460px' },
  readest: { width: 'min(480px, 100%)', height: '360px' },
  'goldendict-mobile': { width: 'min(390px, 100%)', height: '600px' },
  'kobo-pyglossary': { width: 'min(390px, 100%)', height: '600px' },
};

/**
 * CSS pixels per device pixel for a KOReader page: a 300 ppi page shows at
 * about one and a half times its size on the device, and sharp on a
 * screen with two device pixels per CSS pixel.
 */
const PAGE_SCALE = 0.5;

/**
 * Everything in an entry that a click can follow: HTML and SVG `<a>`,
 * image-map `<area>`, and MathML elements with `href`.
 */
const LINKS = 'a, area, math[href], math [href]';
const XLINK = 'http://www.w3.org/1999/xlink';

/** A draw that takes longer than this says so. */
const SLOW_DRAW_MS = 400;

/** How many views Back can return to. */
const HISTORY_LIMIT = 50;

/** The error codes whose preview text is their own; others read as OTHER. */
const PREVIEW_ERRORS = ['LIMIT', 'CRASH', 'LOAD'];

/** The options each select was last given, so unchanged ones stay open. */
const shownOptions = new WeakMap();

/** Gives `select` these [value, label] options and selects `value`. */
function setOptions(select, options, value) {
  const key = JSON.stringify(options);
  if (shownOptions.get(select) !== key) {
    select.replaceChildren(...options.map(([optionValue, label]) => new Option(label, optionValue)));
    shownOptions.set(select, key);
  }
  if (value != null && select.value !== value) select.value = value;
}

function element(tag, props = {}, children = []) {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children);
  return node;
}

/**
 * Draws documents for web-engine readers in a sandboxed frame that runs no
 * script. Each document is parsed inertly, stripped of anything that could
 * load, navigate or run, and imported into the frame; the frame's own CSP
 * allows inline styles and data: images only. Links are handled here.
 */
class FrameView {
  constructor(onLink) {
    this.onLink = onLink;
    this.frame = element('iframe', { className: 'preview-frame' });
    this.frame.setAttribute('sandbox', 'allow-same-origin');
    this.frame.setAttribute('referrerpolicy', 'no-referrer');
    this.ready = new Promise((resolve) => this.frame.addEventListener('load', resolve, { once: true }));
    this.frame.src = new URL('./frame.html', import.meta.url).href;
    this.listening = null;
  }

  get element() {
    return this.frame;
  }

  size(app) {
    Object.assign(this.frame.style, FRAME_SIZES[app] ?? FRAME_SIZES['goldendict-ng']);
  }

  async show(html, scrollTo) {
    await this.ready;
    const doc = this.frame.contentDocument;
    if (this.listening !== doc) {
      // Every activation of a link is cancelled before the frame could
      // navigate; a click hands its target to the reader's rules.
      for (const type of ['click', 'auxclick']) {
        doc.addEventListener(
          type,
          (event) => {
            const link = event.target.closest?.(LINKS);
            if (!link) return;
            event.preventDefault();
            const href = link.getAttribute('href') ?? link.getAttributeNS(XLINK, 'href');
            if (type === 'click' && href !== null) this.onLink(href);
          },
          true,
        );
      }
      this.listening = doc;
    }
    // The frame's own parser: a document parsed by the page's would inherit
    // the page's policy, which forbids the entry's inline styles.
    const parsed = new this.frame.contentWindow.DOMParser().parseFromString(html, 'text/html');
    for (const node of parsed.querySelectorAll('meta, base, link, script, iframe, frame, frameset, object, embed')) {
      node.remove();
    }
    for (const node of parsed.querySelectorAll('[ping]')) node.removeAttribute('ping');
    for (const node of doc.head.querySelectorAll('style')) node.remove();
    for (const style of parsed.head.querySelectorAll('style')) doc.head.append(doc.importNode(style, true));
    doc.body.replaceWith(doc.importNode(parsed.body, true));
    doc.documentElement.scrollTop = 0;
    if (scrollTo) this.scroll(scrollTo);
  }

  scroll(id) {
    const doc = this.frame.contentDocument;
    (doc.getElementById(id) ?? doc.getElementsByName(id)[0])?.scrollIntoView();
  }
}

/**
 * Draws a page MuPDF drew in the worker (KOReader): the image on a canvas,
 * a button over each link, and the page's text for screen readers.
 */
class PageView {
  constructor(onLink) {
    this.onLink = onLink;
    this.canvas = element('canvas', { className: 'preview-canvas' });
    this.canvas.setAttribute('aria-hidden', 'true');
    this.text = element('div', { className: 'visually-hidden' });
    this.links = element('div', { className: 'preview-links' });
    this.element = element('div', { className: 'preview-page' }, [this.canvas, this.text, this.links]);
  }

  show({ width, height, pixels, links, text }) {
    this.canvas.width = width;
    this.canvas.height = height;
    this.canvas.getContext('2d').putImageData(new ImageData(pixels, width, height), 0, 0);
    this.element.style.width = `min(100%, ${width * PAGE_SCALE}px)`;
    const percent = (value, whole) => `${(value / whole) * 100}%`;
    this.links.replaceChildren(
      ...links.map(({ rect: [x0, y0, x1, y1], uri, label }) => {
        const link = element('button', { type: 'button', className: 'preview-link' });
        link.setAttribute('aria-label', label || uri);
        Object.assign(link.style, {
          left: percent(x0, width),
          top: percent(y0, height),
          width: percent(x1 - x0, width),
          height: percent(y1 - y0, height),
        });
        link.addEventListener('click', () => this.onLink(uri));
        return link;
      }),
    );
    this.text.textContent = text;
  }
}

/** A "‹ label ›" row of two buttons around a label. */
function stepper(onStep) {
  const previous = element('button', { type: 'button', className: 'quiet', textContent: '‹' });
  const next = element('button', { type: 'button', className: 'quiet', textContent: '›' });
  const label = element('span');
  previous.addEventListener('click', () => onStep(-1));
  next.addEventListener('click', () => onStep(1));
  return { previous, next, label, row: element('div', {}, [previous, label, next]) };
}

/** One preview: its worker, its history of views and its part of the page. */
export class PreviewPanel {
  constructor({ container, text, zip, reader }) {
    this.container = container;
    this.text = text;
    this.worker = new Worker(new URL('./preview-worker.js', import.meta.url), { type: 'module' });
    this.pending = new Map();
    this.nextId = 1;
    this.latest = 0;
    this.drawing = 0;
    this.worker.onmessage = ({ data }) => {
      const request = this.pending.get(data.id);
      this.pending.delete(data.id);
      if (data.ok) request?.resolve(data.value);
      else request?.reject(data);
    };
    this.worker.onerror = (event) => {
      event.preventDefault();
      this.fail({ code: 'LOAD', detail: event.message });
    };
    this.info = null;
    this.choices = null;
    this.app = null;
    /** The view shown, which of its results, and (KOReader) which page. */
    this.view = null;
    this.result = 0;
    this.page = 0;
    this.pages = 1;
    this.fontsMissing = false;
    /** A draw has taken longer than SLOW_DRAW_MS. */
    this.slow = false;
    this.history = [];
    this.status = { key: 'loadingPreview' };
    this.build();
    this.render();
    this.request('open', { zip, reader })
      .then(({ info, choices }) => {
        this.info = info;
        this.choices = choices;
        this.apps = info.apps;
        this.app = this.apps[0] ?? null;
        this.screen = choices.koreader.screens[0].id;
        this.fontSize = choices.koreader.fontSizes.default;
        this.status = this.app ? null : { key: 'previewUnavailable' };
        this.render();
        if (this.app) this.random();
      })
      .catch((error) => this.fail(error));
  }

  request(type, payload = {}) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.worker.postMessage({ id, type, ...payload });
    });
  }

  /** 'mupdf' or 'web': what draws the current reader. */
  engine() {
    return this.app && this.choices?.apps[this.app].engine;
  }

  /**
   * Whether `view` is drawn one result at a time (KOReader, whose results
   * the worker composes and draws on demand) rather than as one document.
   */
  oneAtATime(view) {
    return this.choices.apps[view.app].engine === 'mupdf';
  }

  /** How many results of the view can be stepped through. */
  steps(view) {
    return this.oneAtATime(view) ? view.results.length : 1;
  }

  build() {
    this.title = element('h3');
    this.appLabel = element('label', { htmlFor: 'preview-app' });
    this.appSelect = element('select', { id: 'preview-app' });
    this.appSelect.addEventListener('change', () => {
      // The new reader looks the same query up; until it answers, and if it
      // finds nothing, nothing of the previous reader stays on screen.
      const previous = this.view;
      this.app = this.appSelect.value;
      this.clearView();
      this.render();
      if (previous) {
        this.search(previous.query, { followed: previous.followed, label: this.word(previous) });
      }
    });
    this.appRow = element('div', { className: 'field' }, [this.appLabel, this.appSelect]);
    this.screenLabel = element('label', { htmlFor: 'preview-screen' });
    this.screenSelect = element('select', { id: 'preview-screen' });
    this.screenSelect.addEventListener('change', () => {
      this.screen = this.screenSelect.value;
      this.redraw();
    });
    this.fontLabel = element('label', { htmlFor: 'preview-font-size' });
    this.fontInput = element('input', { type: 'number', id: 'preview-font-size', inputMode: 'numeric' });
    this.fontInput.addEventListener('change', () => {
      const { min, max } = this.choices.koreader.fontSizes;
      const size = Math.round(Number(this.fontInput.value));
      if (Number.isFinite(size)) this.fontSize = Math.min(Math.max(size, min), max);
      this.fontInput.value = this.fontSize;
      this.redraw();
    });
    this.options = element('div', { className: 'preview-options' }, [
      element('div', { className: 'field' }, [this.screenLabel, this.screenSelect]),
      element('div', { className: 'field' }, [this.fontLabel, this.fontInput]),
    ]);
    this.fidelity = element('p', { className: 'hint' });
    this.input = element('input', { type: 'search', id: 'preview-word', autocomplete: 'off' });
    this.input.setAttribute('list', 'preview-words');
    this.words = element('datalist', { id: 'preview-words' });
    this.input.addEventListener('input', () => this.suggest());
    this.searchLabel = element('label', { htmlFor: 'preview-word', className: 'visually-hidden' });
    this.lookUp = element('button', { type: 'submit' });
    this.randomButton = element('button', { type: 'button', className: 'quiet' });
    this.randomButton.addEventListener('click', () => this.random());
    this.backButton = element('button', { type: 'button', className: 'quiet' });
    this.backButton.addEventListener('click', () => this.back());
    this.form = element('form', { className: 'preview-search' }, [
      this.searchLabel,
      this.input,
      this.words,
      this.lookUp,
      this.randomButton,
      this.backButton,
    ]);
    this.form.addEventListener('submit', (event) => {
      event.preventDefault();
      const word = this.input.value.trim();
      if (word) this.search(word);
    });
    this.statusLine = element('p', { className: 'hint', role: 'status' });
    this.frameView = new FrameView((href) => this.follow(href));
    this.pageView = new PageView((href) => this.follow(href));
    this.stage = element('div', { className: 'preview-stage' }, [this.frameView.element, this.pageView.element]);
    this.resultTurner = stepper((step) => this.turnResult(step));
    this.pageTurner = stepper((step) => this.turnPage(step));
    this.pager = element('div', { className: 'preview-pager' }, [this.resultTurner.row, this.pageTurner.row]);
    this.container.replaceChildren(
      this.title,
      this.appRow,
      this.options,
      this.fidelity,
      this.form,
      this.statusLine,
      this.stage,
      this.pager,
    );
  }

  render() {
    const text = this.text();
    const ui = text.ui;
    const appName = this.app ? text.apps[this.app] : '';
    const paged = this.engine() === 'mupdf';
    this.title.textContent = this.app ? format(ui.previewIn, { app: appName }) : ui.preview;
    const apps = this.apps ?? [];
    this.appRow.hidden = apps.length < 2;
    this.appLabel.textContent = ui.previewAs;
    setOptions(
      this.appSelect,
      apps.map((app) => [app, text.apps[app]]),
      this.app,
    );
    this.options.hidden = !paged;
    if (paged) {
      this.screenLabel.textContent = ui.previewDevice;
      setOptions(
        this.screenSelect,
        this.choices.koreader.screens.map(({ id, width, height }) => [
          id,
          format(ui.previewScreen, { name: text.screens[id], width, height }),
        ]),
        this.screen,
      );
      this.fontLabel.textContent = ui.previewFontSize;
      const { min, max } = this.choices.koreader.fontSizes;
      Object.assign(this.fontInput, { min, max });
      // Leave a size being typed alone.
      if (this.fontInput.ownerDocument.activeElement !== this.fontInput) {
        this.fontInput.value = this.fontSize;
      }
    }
    const facts = this.app && this.choices?.apps[this.app];
    this.fidelity.textContent = facts
      ? [
          text.fidelity[facts.fidelity],
          facts.resources ? '' : ui.previewNoImages,
          paged && this.fontsMissing ? ui.previewFontsMissing : '',
        ]
          .filter(Boolean)
          .join(' ')
      : '';
    this.searchLabel.textContent = ui.previewWord;
    this.input.placeholder = ui.previewWord;
    this.lookUp.textContent = ui.lookUp;
    this.randomButton.textContent = ui.randomWord;
    this.backButton.textContent = ui.back;
    this.backButton.disabled = this.history.length === 0;
    for (const control of [this.input, this.lookUp, this.randomButton, this.appSelect, this.screenSelect, this.fontInput]) {
      control.disabled = !this.app;
    }
    this.statusLine.textContent = this.statusText();
    this.stage.hidden = !this.view;
    this.frameView.element.hidden = paged;
    this.pageView.element.hidden = !paged;
    if (this.app && !paged) this.frameView.size(this.app);
    const results = this.view ? this.steps(this.view) : 0;
    this.step(this.resultTurner, ui.previewResult, this.result, results, ui.previousResult, ui.nextResult);
    this.step(this.pageTurner, ui.previewPage, this.page, paged ? this.pages : 0, ui.previousPage, ui.nextPage);
    this.pager.hidden = this.stage.hidden || (this.resultTurner.row.hidden && this.pageTurner.row.hidden);
  }

  step({ previous, next, label, row }, template, index, count, previousName, nextName) {
    row.hidden = count < 2;
    label.textContent = format(template, { i: index + 1, n: count });
    previous.setAttribute('aria-label', previousName);
    next.setAttribute('aria-label', nextName);
    previous.disabled = index <= 0;
    next.disabled = index >= count - 1;
  }

  statusText() {
    const text = this.text();
    if (this.slow) return text.ui.previewDrawing;
    if (this.status?.key === 'previewFailed') {
      const { code, detail } = this.status;
      const reason = text.previewErrors[PREVIEW_ERRORS.includes(code) ? code : 'OTHER'];
      const details = detail && !PREVIEW_ERRORS.includes(code) ? format(text.ui.errorDetail, { detail }) : '';
      return [text.ui.previewFailed, reason, details].filter(Boolean).join(' ');
    }
    if (this.status) {
      const { key, values = {} } = this.status;
      const template = key.startsWith('stays.') ? text.stays[key.slice(6)] : text.ui[key];
      return format(template ?? '', values);
    }
    if (!this.view) return '';
    const n = this.view.results.length;
    return n === 1
      ? format(text.ui.previewOne, { word: this.view.results[0].headword })
      : format(text.ui.previewMany, { n, word: this.word(this.view) });
  }

  fail(error) {
    this.status = { key: 'previewFailed', code: error.code, detail: error.detail };
    this.render();
  }

  /** Forgets the view and the way back to it, and any draw for it. */
  clearView() {
    this.view = null;
    this.history = [];
    this.result = 0;
    this.page = 0;
    this.pages = 1;
    this.status = null;
    this.cancelDraw();
  }

  /** Makes a pending draw's answer, and its slow notice, count for nothing. */
  cancelDraw() {
    this.drawing++;
    this.slow = false;
  }

  /**
   * The word a view shows in the search box: what was typed, or for a
   * followed link, which may look up an internal key, the headword found.
   */
  word(view) {
    return view.followed ? (view.results[0]?.headword ?? view.query) : view.query;
  }

  /**
   * Shows what a lookup or a followed link leads to. `followed`: the query
   * is a link's target, so the box shows the headword found. `label`: the
   * word to name if nothing is found, when not the query itself.
   */
  async apply(promise, { followed = false, label } = {}) {
    const id = ++this.latest;
    let outcome;
    try {
      outcome = await promise;
    } catch (error) {
      if (id === this.latest) this.fail(error);
      return;
    }
    if (id !== this.latest) return;
    switch (outcome.outcome) {
      case 'view':
        if (this.view) {
          this.history.push({ view: this.view, result: this.result, page: this.page });
          if (this.history.length > HISTORY_LIMIT) this.history.shift();
        }
        this.view = { ...outcome, followed };
        this.result = 0;
        this.page = 0;
        this.pages = 1;
        this.status = null;
        this.input.value = this.word(this.view);
        this.render();
        await this.show(this.view.scroll_to);
        break;
      case 'scroll':
        this.frameView.scroll(outcome.id);
        break;
      case 'stay':
        this.status = { key: `stays.${outcome.reason}` };
        this.render();
        break;
      case 'not-found':
        this.status = { key: 'previewNotFound', values: { word: label ?? outcome.word } };
        this.render();
        break;
    }
  }

  /** Draws the current result of the current view. */
  show(scrollTo) {
    if (this.oneAtATime(this.view)) return this.draw();
    this.cancelDraw();
    return this.frameView.show(this.view.documents[0], scrollTo);
  }

  /** Has the worker draw the current page with MuPDF. */
  async draw() {
    this.cancelDraw();
    const id = this.drawing;
    const slow = setTimeout(() => {
      if (id !== this.drawing) return;
      this.slow = true;
      this.render();
    }, SLOW_DRAW_MS);
    let drawn;
    try {
      drawn = await this.request('draw', {
        query: this.view.query,
        entry: this.view.results[this.result].entry,
        n: this.result,
        screen: this.screen,
        fontSize: this.fontSize,
        page: this.page,
      });
    } catch (error) {
      if (id === this.drawing) {
        this.slow = false;
        this.fail(error);
      }
      return;
    } finally {
      clearTimeout(slow);
    }
    if (id !== this.drawing) return;
    this.slow = false;
    this.page = drawn.page;
    this.pages = drawn.pages;
    this.fontsMissing = drawn.missing.length > 0;
    this.pageView.show(drawn);
    this.render();
  }

  /** Lays the current result out again from its first page. */
  redraw() {
    this.page = 0;
    this.pages = 1;
    this.status = null;
    this.render();
    if (this.view && this.oneAtATime(this.view)) this.draw();
  }

  turnResult(step) {
    this.result = Math.min(Math.max(this.result + step, 0), this.steps(this.view) - 1);
    this.page = 0;
    this.pages = 1;
    this.status = null;
    this.render();
    this.show();
  }

  turnPage(step) {
    this.page = Math.min(Math.max(this.page + step, 0), this.pages - 1);
    this.status = null;
    this.render();
    this.draw();
  }

  search(word, options) {
    return this.apply(this.request('search', { app: this.app, word }), options);
  }

  follow(href) {
    // Readers that draw one result at a time follow links from that result.
    const shown = this.view && this.oneAtATime(this.view) ? this.result : 0;
    const current = this.view?.results[shown]?.entry ?? 0;
    return this.apply(this.request('follow', { app: this.app, href, current }), { followed: true });
  }

  async random() {
    const word = await this.request('random').catch(() => null);
    if (word) this.search(word);
  }

  back() {
    const previous = this.history.pop();
    if (!previous) return;
    ({ view: this.view, result: this.result, page: this.page } = previous);
    this.pages = 1;
    this.status = null;
    this.input.value = this.word(this.view);
    this.render();
    this.show(this.view.scroll_to);
  }

  async suggest() {
    const prefix = this.input.value.trim();
    if (!prefix || !this.app) return;
    const words = await this.request('suggest', { prefix }).catch(() => []);
    if (this.input.value.trim() !== prefix) return;
    this.words.replaceChildren(...words.map((word) => new Option(word)));
  }

  close() {
    this.worker.terminate();
    this.pending.clear();
    this.container.replaceChildren();
  }
}
