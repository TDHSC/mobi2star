// The preview panel: search the converted dictionary and follow its links,
// drawn as the chosen reader draws them. Lookups and link rules run in the
// preview worker (Rust); this file only draws what comes back.
import { format } from './i18n.js';

/** Frame sizes in CSS pixels: a desktop window, a popup, or a phone. */
const FRAME_SIZES = {
  'goldendict-ng': { width: '100%', height: '460px' },
  readest: { width: 'min(480px, 100%)', height: '360px' },
  'goldendict-mobile': { width: 'min(390px, 100%)', height: '600px' },
  'kobo-pyglossary': { width: 'min(390px, 100%)', height: '600px' },
};

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

  size(app) {
    Object.assign(this.frame.style, FRAME_SIZES[app] ?? FRAME_SIZES['goldendict-ng']);
  }

  async show(html, scrollTo) {
    await this.ready;
    const doc = this.frame.contentDocument;
    if (this.listening !== doc) {
      doc.addEventListener(
        'click',
        (event) => {
          const link = event.target.closest?.('a[href]');
          if (!link) return;
          event.preventDefault();
          this.onLink(link.getAttribute('href'));
        },
        true,
      );
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

/** One preview: its worker, its history of views and its part of the page. */
export class PreviewPanel {
  constructor({ container, text, zip, reader }) {
    this.container = container;
    this.text = text;
    this.worker = new Worker(new URL('./preview-worker.js', import.meta.url), { type: 'module' });
    this.pending = new Map();
    this.nextId = 1;
    this.latest = 0;
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
    this.view = null;
    this.history = [];
    this.status = { key: 'loadingPreview' };
    this.build();
    this.render();
    this.request('open', { zip, reader })
      .then(({ info, choices }) => {
        this.info = info;
        this.choices = choices;
        this.apps = info.apps.filter((app) => choices.apps[app].engine === 'web');
        this.app = this.apps[0] ?? null;
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

  build() {
    this.title = element('h3');
    this.appLabel = element('label', { htmlFor: 'preview-app' });
    this.appSelect = element('select', { id: 'preview-app' });
    this.appSelect.addEventListener('change', () => {
      this.app = this.appSelect.value;
      this.history = [];
      this.render();
      if (this.view) this.search(this.view.query);
    });
    this.appRow = element('div', { className: 'field' }, [this.appLabel, this.appSelect]);
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
    this.stage = element('div', { className: 'preview-stage' }, [this.frameView.frame]);
    this.container.replaceChildren(
      this.title,
      this.appRow,
      this.fidelity,
      this.form,
      this.statusLine,
      this.stage,
    );
  }

  render() {
    const text = this.text();
    const ui = text.ui;
    const appName = this.app ? text.apps[this.app] : '';
    this.title.textContent = this.app ? format(ui.previewIn, { app: appName }) : ui.preview;
    const apps = this.apps ?? [];
    this.appRow.hidden = apps.length < 2;
    this.appLabel.textContent = ui.previewAs;
    const selected = this.app;
    this.appSelect.replaceChildren(...apps.map((app) => new Option(text.apps[app], app)));
    if (selected) this.appSelect.value = selected;
    const facts = this.app && this.choices?.apps[this.app];
    this.fidelity.textContent = facts
      ? [text.fidelity[facts.fidelity], facts.resources ? '' : ui.previewNoImages].filter(Boolean).join(' ')
      : '';
    this.searchLabel.textContent = ui.previewWord;
    this.input.placeholder = ui.previewWord;
    this.lookUp.textContent = ui.lookUp;
    this.randomButton.textContent = ui.randomWord;
    this.backButton.textContent = ui.back;
    this.backButton.disabled = this.history.length === 0;
    for (const control of [this.input, this.lookUp, this.randomButton, this.appSelect]) {
      control.disabled = !this.app;
    }
    this.statusLine.textContent = this.statusText();
    this.stage.hidden = !this.view;
    if (this.app) this.frameView.size(this.app);
  }

  statusText() {
    const text = this.text();
    if (this.status) {
      const { key, values = {} } = this.status;
      const template = key.startsWith('stays.') ? text.stays[key.slice(6)] : text.ui[key];
      return format(template ?? '', values);
    }
    if (!this.view) return '';
    const n = this.view.results.length;
    return n === 1
      ? format(text.ui.previewOne, { word: this.view.results[0].headword })
      : format(text.ui.previewMany, { n, word: this.view.query });
  }

  fail(error) {
    const errors = this.text().errors;
    this.status = { key: 'previewFailed' };
    this.render();
    this.statusLine.textContent = `${this.statusText()} ${errors[error.code] ?? errors.OTHER}`;
  }

  async apply(promise) {
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
        if (this.view) this.history.push(this.view);
        this.view = outcome;
        this.status = null;
        this.input.value = outcome.query;
        this.render();
        await this.frameView.show(outcome.documents[0], outcome.scroll_to);
        break;
      case 'scroll':
        this.frameView.scroll(outcome.id);
        break;
      case 'stay':
        this.status = { key: `stays.${outcome.reason}` };
        this.render();
        break;
      case 'not-found':
        this.status = { key: 'previewNotFound', values: { word: outcome.word } };
        this.render();
        break;
    }
  }

  search(word) {
    return this.apply(this.request('search', { app: this.app, word }));
  }

  follow(href) {
    const current = this.view?.results[0]?.entry ?? 0;
    return this.apply(this.request('follow', { app: this.app, href, current }));
  }

  async random() {
    const word = await this.request('random').catch(() => null);
    if (word) this.search(word);
  }

  back() {
    const previous = this.history.pop();
    if (!previous) return;
    this.view = previous;
    this.status = null;
    this.input.value = previous.query;
    this.render();
    this.frameView.show(previous.documents[0], previous.scroll_to);
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
