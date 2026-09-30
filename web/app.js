// The page: choosing a file and options, one worker per conversion, and
// showing progress, errors and the result. All text comes from i18n.js and
// is set with textContent.
import { LANGUAGES, TEXT, format } from './i18n.js';

const $ = (id) => document.getElementById(id);
const STORAGE_KEY = 'mobi2star.language';
const readers = Object.keys(TEXT.en.readers);
const version = $('version').dataset.version;

const state = {
  lang: initialLanguage(),
  file: null,
  /** The labels choice follows the page language until the user picks one. */
  labelsChosen: false,
  worker: null,
  stage: null,
  notice: null,
  error: null,
  result: null,
  url: null,
};

function initialLanguage() {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (LANGUAGES.includes(saved)) return saved;
  } catch {
    // Storage can be unavailable; fall back to the browser language.
  }
  return navigator.language?.toLowerCase().startsWith('zh') ? 'zh' : 'en';
}

const text = () => TEXT[state.lang];
const lookup = (path) => path.split('.').reduce((node, key) => node?.[key], text());
const element = (tag, content) => Object.assign(document.createElement(tag), { textContent: content });
const number = (n) => new Intl.NumberFormat(state.lang).format(n);

function size(bytes) {
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  const digits = unit === 0 ? 0 : 1;
  return `${new Intl.NumberFormat(state.lang, { maximumFractionDigits: digits }).format(value)} ${units[unit]}`;
}

function fillSelect(select, options) {
  const value = select.value;
  select.replaceChildren(...options.map(([key, label]) => new Option(label, key)));
  if (value) select.value = value;
}

function render() {
  const ui = text().ui;
  document.documentElement.lang = state.lang === 'zh' ? 'zh-CN' : 'en';
  for (const node of document.querySelectorAll('[data-text]')) {
    node.textContent = lookup(node.dataset.text);
  }
  $('language').textContent = ui.toggle;
  $('language').setAttribute('aria-label', ui.toggleLabel);
  $('version').textContent = format(ui.version, { version });
  fillSelect($('reader'), readers.map((reader) => [reader, text().readers[reader].name]));
  fillSelect($('labels'), Object.entries(text().labels));
  if (!state.labelsChosen) $('labels').value = state.lang;
  $('reader-description').textContent = text().readers[$('reader').value].description;
  $('chosen').textContent = state.file
    ? format(ui.chosen, { name: state.file.name, size: size(state.file.size) })
    : '';
  $('notice').hidden = !state.notice;
  $('notice').textContent = state.notice ? ui[state.notice] : '';
  renderControls();
  renderStage();
  renderError();
  renderResult();
}

function renderControls() {
  const busy = state.worker !== null;
  $('convert').disabled = busy || !state.file;
  $('cancel').hidden = !busy;
  for (const id of ['file', 'reader', 'labels']) $(id).disabled = busy;
}

function renderStage() {
  const { stage } = state;
  $('progress').hidden = !stage;
  if (!stage) return;
  const bar = $('bar');
  if (stage.stage === 'rendering') {
    bar.max = stage.total;
    bar.value = stage.done;
  } else if (stage.stage === 'starting' || stage.stage === 'parsing') {
    bar.removeAttribute('value');
  }
  const values = stage.stage === 'rendering' ? { done: number(stage.done), total: number(stage.total) } : {};
  $('stage').textContent = format(text().stages[stage.stage] ?? '', values);
}

function renderError() {
  const { error } = state;
  $('error').hidden = !error;
  if (!error) return;
  const errors = text().errors;
  $('error-title').textContent = errors[error.code] ?? errors.OTHER;
  $('error-detail').textContent = error.detail ? format(text().ui.errorDetail, { detail: error.detail }) : '';
}

function renderResult() {
  const { result } = state;
  $('result').hidden = !result;
  if (!result) return;
  const ui = text().ui;
  const { summary } = result;
  const { report } = summary;
  const rows = [
    [ui.dictionary, summary.folder],
    [ui.headwords, number(report.source_headwords)],
    [ui.aliases, number(report.source_aliases)],
    [ui.entries, number(report.output_entries)],
    [ui.source, 'backend' in report ? ui.sourceSrcs : ui.sourceCompiled],
  ];
  $('summary').replaceChildren(...rows.flatMap(([term, value]) => [element('dt', term), element('dd', value)]));
  const link = $('download');
  link.href = state.url;
  link.download = result.fileName;
  link.textContent = format(ui.download, { name: result.fileName, size: size(result.zip.size) });
  $('install').replaceChildren(...text().readers[result.reader].install.map((step) => element('li', step)));
  // Publisher-source reports list their checks; compiled reports have notes
  // on what the checks do and do not cover.
  const checks = report.verification_scope;
  $('checks-title').textContent = checks ? ui.details : ui.notes;
  $('checks').replaceChildren(...(checks ?? report.notes).map((line) => element('li', line)));
}

/** Clears the previous outcome and frees its download. */
function clearOutcome() {
  if (state.url) URL.revokeObjectURL(state.url);
  Object.assign(state, { url: null, result: null, error: null, notice: null });
}

function choose(file) {
  if (!file || state.worker) return;
  state.file = file;
  clearOutcome();
  render();
}

/** Stops the worker, which frees all of its memory. */
function stopWorker() {
  state.worker?.terminate();
  state.worker = null;
  state.stage = null;
}

function convert(event) {
  event.preventDefault();
  if (!state.file || state.worker) return;
  clearOutcome();
  const choices = { reader: $('reader').value, labels: $('labels').value };
  const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
  worker.onmessage = ({ data }) => {
    if (data.type === 'progress') {
      state.stage = data.stage;
      renderStage();
      return;
    }
    stopWorker();
    if (data.type === 'done') {
      state.url = URL.createObjectURL(data.zip);
      state.result = { ...data, reader: choices.reader };
    } else {
      state.error = { code: data.code, detail: data.detail };
    }
    render();
  };
  worker.onerror = (event) => {
    event.preventDefault();
    stopWorker();
    state.error = { code: 'LOAD', detail: event.message };
    render();
  };
  state.worker = worker;
  state.stage = { stage: 'starting' };
  worker.postMessage({ file: state.file, choices });
  render();
}

function cancel() {
  stopWorker();
  state.notice = 'cancelled';
  render();
}

$('form').addEventListener('submit', convert);
$('cancel').addEventListener('click', cancel);
$('file').addEventListener('change', () => choose($('file').files[0]));
$('reader').addEventListener('change', render);
$('labels').addEventListener('change', () => {
  state.labelsChosen = true;
});
$('language').addEventListener('click', () => {
  state.lang = state.lang === 'en' ? 'zh' : 'en';
  try {
    localStorage.setItem(STORAGE_KEY, state.lang);
  } catch {
    // The choice then lasts only for this visit.
  }
  render();
});

const drop = $('drop');
drop.addEventListener('dragover', (event) => {
  event.preventDefault();
  drop.classList.add('over');
});
drop.addEventListener('dragleave', () => drop.classList.remove('over'));
drop.addEventListener('drop', (event) => {
  event.preventDefault();
  drop.classList.remove('over');
  choose(event.dataTransfer.files[0]);
});
// A file dropped beside the drop zone must not replace the page.
window.addEventListener('dragover', (event) => event.preventDefault());
window.addEventListener('drop', (event) => event.preventDefault());

render();
