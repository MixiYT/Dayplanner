'use strict';
/* Интерфейс ежедневника. Данные и логика живут в ядре (Rust): здесь только показ и ввод. */

// ---------- мост к Rust и к Android ----------
const TAURI = window.__TAURI__;
const IS_DEMO = !TAURI;
const IS_ANDROID = document.documentElement.dataset.platform === 'android';

/* Время всегда берём с часов устройства: на Android ядро может не знать часовой пояс. */
const localIso = () => { const d = new Date(); return `${ymd(d)}T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`; };
async function api(cmd, args = {}, now = null) {
  if (IS_DEMO) return demo.api(cmd, args, now);
  return TAURI.core.invoke('api', { cmd, args, now: now || localIso() });
}
async function native(cmd, args = {}) {
  if (IS_DEMO) return null;
  try { return await TAURI.core.invoke('plugin:planner-native|native', { cmd, args }); } catch (_) { return null; }
}
function errText(e) { return typeof e === 'string' ? e : (e && e.message) || 'Что-то пошло не так'; }

let insetRequest = 0;
async function updatePlatformInsets() {
  if (IS_DEMO || !IS_ANDROID) return;
  const request = ++insetRequest, insets = await native('insets');
  if (request !== insetRequest || !insets || !(insets.width > 0)) return;
  const scale = document.documentElement.clientWidth / insets.width;
  for (const edge of ['top', 'right', 'bottom', 'left']) {
    const value = Number(insets[edge]);
    if (Number.isFinite(value) && value >= 0) document.documentElement.style.setProperty(`--safe-${edge}`, `${value * scale}px`);
  }
}
window.addEventListener('resize', updatePlatformInsets);
window.addEventListener('planner-insets', updatePlatformInsets);
window.addEventListener('focus', updatePlatformInsets);

// ---------- утилиты ----------
const pad = (n) => String(n).padStart(2, '0');
const ymd = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const fromYmd = (s) => { const [y, m, d] = s.split('-').map(Number); return new Date(y, m - 1, d); };
const addDays = (d, n) => { const x = new Date(d); x.setDate(x.getDate() + n); return x; };
const todayStr = () => ymd(new Date());
const hhmm = (iso) => iso.slice(11, 16);
const toMin = (t) => { const [h, m] = t.split(':').map(Number); return h * 60 + m; };
const WEEKDAYS = ['Понедельник', 'Вторник', 'Среда', 'Четверг', 'Пятница', 'Суббота', 'Воскресенье'];
const WD_SHORT = ['Пн', 'Вт', 'Ср', 'Чт', 'Пт', 'Сб', 'Вс'];
const longDate = (d) => { const t = d.toLocaleDateString('ru-RU', { weekday: 'long', day: 'numeric', month: 'long' }); return t.charAt(0).toUpperCase() + t.slice(1); };
function plural(n, [a, b, c]) {
  const m = Math.abs(n) % 100, k = m % 10;
  return m > 10 && m < 20 ? c : k === 1 ? a : k >= 2 && k <= 4 ? b : c;
}

function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === null || v === undefined || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'style') el.style.cssText = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'value') el.value = v;
    else if (v === true) el.setAttribute(k, '');
    else el.setAttribute(k, v);
  }
  const add = (c) => {
    if (c === null || c === undefined || c === false) return;
    if (Array.isArray(c)) c.forEach(add);
    else el.append(c.nodeType ? c : document.createTextNode(String(c)));
  };
  kids.forEach(add);
  return el;
}

const ICONS = {
  today: '<path d="M7 3v3M17 3v3M4 9h16M5 5h14a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z"/><path d="M8 14l2.5 2.5L16 12"/>',
  list: '<path d="M9 6h11M9 12h11M9 18h11"/><circle cx="4.5" cy="6" r="1"/><circle cx="4.5" cy="12" r="1"/><circle cx="4.5" cy="18" r="1"/>',
  trophy: '<path d="M8 4h8v5a4 4 0 0 1-8 0V4z"/><path d="M8 6H5v1a3 3 0 0 0 3 3M16 6h3v1a3 3 0 0 1-3 3M12 13v4M8 20h8M10 17h4"/>',
  note: '<path d="M6 3h9l4 4v13a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z"/><path d="M15 3v4h4M8.5 12h7M8.5 16h5"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4L7 17M17 7l1.4-1.4"/>',
};
function svg(name) {
  const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  s.setAttribute('viewBox', '0 0 24 24'); s.setAttribute('aria-hidden', 'true'); s.innerHTML = ICONS[name];
  return FeatureUI.icon(name, s);
}

// ---------- состояние ----------
const S = {
  tab: 'today', view: 'day', date: todayStr(),
  settings: {}, cats: [], filterCat: null, noteQuery: '', noteCat: null, noteQuery: '', noteCat: null,
  board: { period: 'week', roomOnly: false }, token: 0,
};
const catById = (id) => S.cats.find((c) => c.id === id);
const catColor = (id) => (catById(id) || {}).color || getComputedStyle(document.documentElement).getPropertyValue('--accent').trim() || '#6750A4';

const EMOJI = ['🔔', '⏰', '💊', '💼', '📞', '📧', '🛒', '🏋️', '🏃', '🧘', '📖', '🎓', '✍️', '💻', '🧹', '🍳', '🚗', '✈️', '🎂', '🎁', '💳', '🏠', '🐕', '🌱',
  '❤️', '⭐', '🎯', '🎵', '🎮', '☕', '💧', '😴', '🦷', '🩺', '🧾', '📅', '📦', '🛠️', '🔑', '👶'];
const ACCENTS = ['#6750A4', '#1A73E8', '#009688', '#2E9E5B', '#F9A825', '#F4511E', '#E91E63', '#8E24AA', '#546E7A'];
const THEMES = [
  ['material', 'Material', 'Мягкие тона от акцента'],
  ['amoled', 'AMOLED', 'Чистый чёрный'],
  ['glass', 'Liquid glass', 'Стекло и размытие'],
  ['simple', 'Simple', 'Спокойно и плоско'],
  ['nord', 'Nord', 'Северная палитра'],
  ['mono', 'Minimal', 'Нейтральные поверхности'],
];
const STATUS_TEXT = {
  upcoming: 'Впереди', due: 'Пора выполнить', done_on_time: 'Выполнено вовремя', done_late: 'Выполнено с опозданием',
  missed: 'Не выполнено', skipped: 'Пропущено',
};
const ALERT_TEXT = { notification: 'Уведомление', alarm: 'Будильник', full_screen: 'На весь экран' };
const ALERT_ICON = { notification: '🔔', alarm: '⏰', full_screen: '📱' };

// ---------- оформление ----------
const darkQuery = window.matchMedia('(prefers-color-scheme: dark)');
function luminance(hex) {
  const n = parseInt(hex.slice(1), 16);
  const f = (c) => { c /= 255; return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
  return 0.2126 * f((n >> 16) & 255) + 0.7152 * f((n >> 8) & 255) + 0.0722 * f(n & 255);
}
function applyTheme() {
  const s = S.settings;
  const theme = s.theme || 'material';
  const accent = /^#[0-9a-f]{6}$/i.test(s.accent || '') ? s.accent : '#6750A4';
  let scheme = s.mode === 'light' ? 'light' : s.mode === 'dark' ? 'dark' : (darkQuery.matches ? 'dark' : 'light');
  if (theme === 'amoled') scheme = 'dark';
  const r = document.documentElement;
  r.dataset.theme = theme; r.dataset.scheme = scheme;
  r.style.setProperty('--accent', accent);
  r.style.setProperty('--on-accent', luminance(accent) > 0.4 ? '#161616' : '#ffffff');
  Background.set({ mode: s.bg || 'none', intensity: (Number(s.bg_intensity) || 0) / 100, accent, dark: scheme === 'dark' });
  FeatureUI.apply(s);
}
darkQuery.addEventListener?.('change', () => { if ((S.settings.mode || 'auto') === 'auto') applyTheme(); });

async function setSetting(key, value, rerender = false) {
  S.settings[key] = value;
  await api('settings.set', { key, value: String(value) });
  applyTheme();
  syncSoon();
  if (rerender) render();
}

// ---------- сообщения ----------
function toast(text, kind = '') {
  const t = h('div', { class: `toast ${kind}` }, text);
  $('#toast-root').append(t);
  setTimeout(() => t.remove(), kind === 'gold' ? 4200 : 2600);
}
const $ = (s, r = document) => r.querySelector(s);

// ---------- связь с Android: будильники и виджеты ----------
let syncTimer = 0;
function syncSoon() { clearTimeout(syncTimer); syncTimer = setTimeout(syncNative, 400); }
async function syncNative() {
  if (IS_DEMO) return;
  try {
    const [alarms, widget] = await Promise.all([api('alarms', { days: 30, limit: 200 }), api('widget')]);
    await native('sync', { alarms: JSON.stringify(alarms), widget: JSON.stringify(widget) });
  } catch (_) { /* не критично: повторим при следующем изменении */ }
}
/* Действия «Выполнено»/«Пропустить», нажатые в уведомлении, пока приложение было закрыто. */
async function drainNativeActions() {
  const res = await native('drain');
  const items = (res && res.items) || [];
  let changed = false;
  for (const a of items) {
    try { await api(a.kind === 'skip' ? 'skip' : 'complete', { reminder_id: a.reminder_id, at: a.at }, a.ts || null); changed = true; } catch (_) { /* уже отмечено */ }
  }
  if (changed) { syncSoon(); pushScoreSoon(); }
  return changed;
}
let pushTimer = 0;
function pushScoreSoon() {
  const s = S.settings;
  if (IS_DEMO || s['online.registered'] !== 'true' || !(s['online.server'] || '').trim()) return;
  clearTimeout(pushTimer);
  pushTimer = setTimeout(() => api('online.push').catch(() => {}), 1500);
}

// ---------- общие действия ----------
async function doComplete(item) {
  try {
    const out = await api('complete', { reminder_id: item.reminder_id, at: item.at });
    toast(`+${out.points} ${plural(out.points, ['очко', 'очка', 'очков'])}`);
    out.unlocked.forEach((a) => toast(`${a.icon} Достижение: ${a.title} (+${a.reward})`, 'gold'));
    if (navigator.vibrate) navigator.vibrate(out.unlocked.length ? [30, 40, 30] : 20);
    syncSoon(); pushScoreSoon();
  } catch (e) { toast(errText(e), 'bad'); }
  render();
}
async function doUndo(item) {
  try { await api('undo', { reminder_id: item.reminder_id, at: item.at }); syncSoon(); pushScoreSoon(); } catch (e) { toast(errText(e), 'bad'); }
  render();
}
async function doSkip(item) {
  try { await api('skip', { reminder_id: item.reminder_id, at: item.at }); syncSoon(); } catch (e) { toast(errText(e), 'bad'); }
  render();
}

// ---------- нижний лист ----------
function openSheet(title, body, onClose) {
  const previousFocus = document.activeElement;
  let closed = false;
  const close = () => { if (closed) return; closed = true; back.remove(); sheet.remove(); document.removeEventListener('keydown', esc); previousFocus?.focus?.(); if (onClose) onClose(); };
  const esc = (e) => { if (e.key === 'Escape' && [...document.querySelectorAll('.sheet')].at(-1) === sheet) { e.stopImmediatePropagation(); close(); } };
  const back = h('div', { class: 'backdrop', onclick: close });
  const sheet = h('div', { class: 'sheet', role: 'dialog', 'aria-modal': 'true', 'aria-label': title },
    h('div', { class: 'sheet-head' }, h('h2', {}, title), h('button', { class: 'icon-btn', 'aria-label': 'Закрыть', onclick: close }, '✕')),
    h('div', {class: 'sheet-body'}, body));
  document.body.append(back, sheet);
  document.addEventListener('keydown', esc);
  sheet.querySelector('button')?.focus();
  return close;
}
function confirmSheet(text, action, onYes) {
  const close = openSheet('Подтвердите', h('div', { class: 'stack' },
    h('p', {}, text),
    h('div', { class: 'actions' },
      h('button', { class: 'btn danger', onclick: async () => { close(); await onYes(); } }, action),
      h('button', { class: 'btn', onclick: () => close() }, 'Отмена'))));
}

// ---------- шапка и навигация ----------
const TABS = [['today', 'Сегодня', 'today'], ['reminders', 'Дела', 'list'], ['notes', 'Заметки', 'note'], ['progress', 'Успехи', 'trophy'], ['settings', 'Настройки', 'settings']];
function drawChrome() {
  const titles = { today: 'Ежедневник', reminders: 'Все напоминания', notes: 'Заметки', progress: 'Успехи и рейтинг', settings: 'Настройки' };
  $('#top').replaceChildren(h('h1', {}, titles[S.tab]));
  const heading = $('#top h1');
  FeatureUI.undoButton().then(button => { if (heading.isConnected) heading.after(button); });
  $('#nav').replaceChildren(...TABS.map(([id, label, icon]) =>
    h('button', { type: 'button', 'aria-label': label, 'aria-current': S.tab === id ? 'page' : null, onclick: () => { S.tab = id; render(); } },
      h('span', { class: 'nav-icon' }, svg(icon)), h('span', { class: 'nav-label' }, label))));
}

async function render() {
  const token = ++S.token;
  drawChrome();
  const view = $('#view');
  let node;
  try {
    S.cats = await api('categories.list');
    if (S.tab === 'today') node = await viewToday();
    else if (S.tab === 'reminders') node = await viewReminders();
    else if (S.tab === 'notes') node = await viewNotes();
    else if (S.tab === 'progress') node = await viewProgress();
    else node = await viewSettings();
  } catch (e) { node = h('div', { class: 'card empty' }, h('div', { class: 'big' }, '⚠️'), h('p', {}, errText(e))); }
  if (token !== S.token) return;
  const keep = view.scrollTop;
  view.replaceChildren(node);
  view.scrollTop = keep;
  $$('.fab').forEach((f) => f.remove());
  if (S.tab === 'today' || S.tab === 'reminders') {
    document.body.append(h('button', { class: 'fab', 'aria-label': 'Добавить', onclick: addChooser }, FeatureUI.icon('plusS')));
  } else if (S.tab === 'notes') {
    document.body.append(h('button', { class: 'fab', 'aria-label': 'Новая заметка', onclick: () => noteSheet(null) }, FeatureUI.icon('plusS')));
  }
}
const $$ = (s, r = document) => [...r.querySelectorAll(s)];

function addChooser() {
  const close = openSheet('Что добавить?', h('div', { class: 'stack' },
    h('button', { class: 'btn primary', onclick: () => { close(); reminderSheet(null); } }, '🔔 Напоминание'),
    h('button', { class: 'btn', onclick: () => { close(); scheduleSheet(null); } }, '🗓️ Занятие в расписании')));
}

// ---------- вкладка «Заметки» ----------
const NOTE_COLORS = ['#E53935', '#FB8C00', '#F9A825', '#43A047', '#00ACC1', '#1E88E5', '#8E24AA', '#D81B60'];
function noteDate(iso) {
  if (iso.slice(0, 10) === todayStr()) return hhmm(iso);
  return fromYmd(iso.slice(0, 10)).toLocaleDateString('ru-RU', { day: 'numeric', month: 'short' });
}
function noteCard(n) {
  const cat = n.category_id ? catById(n.category_id) : null;
  return h('button', { class: `note${n.color ? ' colored' : ''}`, style: n.color ? `--nc:${n.color}` : null, onclick: () => noteSheet(n) },
    n.pinned ? h('span', { class: 'note-pin', title: 'Закреплена', 'aria-label': 'Закреплена' }, '📌') : null,
    h('span', {class: 'note-heading'}, FeatureUI.recordIcon(n.icon || 'builtin:note'), n.title ? h('span', { class: 'note-title' }, n.title) : null),
    n.body ? h('span', { class: 'note-body' }, n.body) : null,
    n.attachments?.length ? h('span', {class: 'muted small'}, `📎 Вложений: ${n.attachments.length}`) : null,
    h('span', { class: 'note-foot muted small' },
      cat ? h('span', {}, h('i', { class: 'dot', style: `background:${cat.color}` }), ` ${cat.name}`) : h('span', {}),
      h('span', {}, noteDate(n.updated))));
}
async function viewNotes() {
  const grid = h('div', { class: 'notes-grid' });
  let timer = 0, seq = 0;
  const load = async () => {
    const mine = ++seq;
    try {
      const list = await api('notes.list', { query: S.noteQuery.trim() || null, category_id: S.noteCat });
      if (mine !== seq) return;
      const filtered = S.noteQuery.trim() || S.noteCat !== null;
      grid.replaceChildren(...(list.length ? list.map(noteCard) : [h('div', { class: 'card empty notes-empty' },
        h('div', { class: 'big' }, filtered ? '🔍' : '🗒️'),
        h('b', {}, filtered ? 'Ничего не найдено' : 'Заметок пока нет'),
        h('p', {}, filtered ? 'Измените запрос или категорию.' : 'Запишите мысль, список или идею: нажмите «+».'),
        filtered ? null : h('button', { class: 'btn primary', onclick: () => noteSheet(null) }, 'Создать заметку'))]));
    } catch (e) { if (mine === seq) grid.replaceChildren(h('div', { class: 'card empty' }, h('p', {}, errText(e)))); }
  };
  const search = h('input', { class: 'input', type: 'search', placeholder: 'Поиск по заметкам', 'aria-label': 'Поиск по заметкам', value: S.noteQuery,
    oninput: (e) => { S.noteQuery = e.target.value; clearTimeout(timer); timer = setTimeout(load, 200); } });
  const chips = h('div', { class: 'chips scroll' },
    h('button', { class: 'chip', 'aria-pressed': String(S.noteCat === null), onclick: () => { S.noteCat = null; render(); } }, 'Все'),
    ...S.cats.map((c) => h('button', { class: 'chip', 'aria-pressed': String(S.noteCat === c.id), onclick: () => { S.noteCat = c.id; render(); } }, FeatureUI.recordIcon(c.icon), ' ', c.name)));
  await load();
  return h('div', { class: 'stack' }, search, chips, grid);
}

function noteSheet(existing) {
  const n = existing ? { ...existing } : { id: 0, title: '', body: '', color: null, pinned: false, category_id: null };
  const attachments = FeatureUI.attachmentsEditor(n);
  n.icon = n.icon || 'builtin:note';
  const signature = () => JSON.stringify([n.title, n.body, n.icon, n.color, n.pinned, n.category_id, n.attachments.map(f => [f.id, f.name])]);
  const original = signature();
  let finished = false;
  const title = h('input', { class: 'input', value: n.title, placeholder: 'Заголовок', maxlength: '200', 'aria-label': 'Заголовок', autofocus: !existing });
  const body = h('textarea', { class: 'input note-editor', placeholder: 'Текст заметки', rows: '10', maxlength: '20000', 'aria-label': 'Текст заметки' }, n.body);
  const catSel = h('select', { onchange: (e) => { n.category_id = e.target.value ? Number(e.target.value) : null; } },
    h('option', { value: '' }, 'Без категории'),
    ...S.cats.map((c) => h('option', { value: c.id, selected: c.id === n.category_id }, c.name)));
  const swatches = h('div', { class: 'swatches' });
  const paintColors = () => swatches.replaceChildren(
    h('button', { type: 'button', class: 'swatch none', 'aria-pressed': String(!n.color), 'aria-label': 'Без цвета', title: 'Без цвета', onclick: () => { n.color = null; paintColors(); } }, '∅'),
    ...NOTE_COLORS.map((c) => h('button', { type: 'button', class: 'swatch', style: `--c:${c}`, 'aria-pressed': String(n.color === c), 'aria-label': `Цвет ${c}`, onclick: () => { n.color = c; paintColors(); } })));
  const pin = h('button', { type: 'button', class: 'chip', 'aria-pressed': String(n.pinned), onclick: () => { n.pinned = !n.pinned; pin.setAttribute('aria-pressed', String(n.pinned)); } }, '📌 Закрепить наверху');
  paintColors();
  const collect = () => { n.title = title.value; n.body = body.value; };
  const dirty = () => { collect(); return signature() !== original; };
  const empty = () => !n.title.trim() && !n.body.trim();
  const persist = async () => { await attachments.ready(); await api('notes.save', { note: n }); };
  // Закрыли лист крестиком, фоном или Esc: изменения не теряются.
  const onClose = async () => {
    if (finished) return;
    finished = true;
    await attachments.ready();
    if (!dirty() || empty()) return;
    try { await persist(); toast('Заметка сохранена'); } catch (e) { toast(errText(e), 'bad'); }
    render();
  };
  const close = openSheet(existing ? 'Заметка' : 'Новая заметка', h('div', { class: 'stack' },
    title, body, field('Иконка', iconPicker(n.icon, value => { n.icon = value; })), attachments,
    h('div', { class: 'field' }, 'Цвет', swatches),
    h('div', { class: 'row' }, pin),
    field('Категория', catSel),
    h('div', { class: 'actions' },
      h('button', { class: 'btn primary', onclick: async () => {
        collect();
        if (empty()) { toast('Заметка пуста: добавьте заголовок или текст', 'bad'); return; }
        try { await persist(); finished = true; close(); toast('Сохранено'); render(); } catch (e) { toast(errText(e), 'bad'); }
      } }, 'Сохранить'),
      existing ? h('button', { class: 'btn danger', onclick: () => confirmSheet('Удалить заметку? Это нельзя отменить.', 'Удалить', async () => {
        try { await api('notes.delete', { id: n.id }); finished = true; close(); toast('Заметка удалена'); render(); } catch (e) { toast(errText(e), 'bad'); }
      }) }, 'Удалить') : null)), onClose);
}

// ---------- вкладка «Сегодня» ----------
async function viewToday() {
  const date = S.date;
  const isToday = date === todayStr();
  const d = fromYmd(date);
  const step = S.view === 'week' ? 7 : 1;
  const bar = h('div', { class: 'datebar' },
    h('button', { class: 'icon-btn', 'aria-label': 'Назад', onclick: () => { S.date = ymd(addDays(d, -step)); render(); } }, '‹'),
    h('h2', {}, S.view === 'week' ? 'Неделя' : longDate(d)),
    !isToday || S.view === 'week' ? h('button', { class: 'btn small', onclick: () => { S.date = todayStr(); render(); } }, 'Сегодня') : null,
    h('button', { class: 'icon-btn', 'aria-label': 'Вперёд', onclick: () => { S.date = ymd(addDays(d, step)); render(); } }, '›'));
  const seg = h('div', { class: 'seg', role: 'radiogroup', 'aria-label': 'Вид' },
    ...[['day', 'День'], ['week', 'Неделя']].map(([v, l]) =>
      h('button', { role: 'radio', 'aria-checked': String(S.view === v), onclick: () => { S.view = v; render(); } }, l)));
  const body = S.view === 'week' ? await weekBody() : await dayBody();
  return h('div', { class: 'stack' }, bar, seg, body);
}

function itemRow(i, { showDate = false } = {}) {
  const done = i.status === 'done_on_time' || i.status === 'done_late';
  return h('div', { class: 'item', 'data-status': i.status },
    h('button', { class: 'check', 'aria-pressed': String(done), 'aria-label': done ? 'Снять отметку' : 'Отметить выполненным', onclick: () => (done ? doUndo(i) : doComplete(i)) }, done ? '✓' : ''),
    h('button', { class: 'item grow', style: 'padding:0;background:none;border:0;border-top:0', onclick: () => itemSheet(i) },
      h('span', { class: 'ico' }, FeatureUI.recordIcon(i.icon)),
      h('span', { class: 'grow' },
        h('span', { class: 'title', style: done ? 'text-decoration:line-through;opacity:.65' : '' }, i.title),
        h('span', { class: 'muted small', style: 'display:block' },
          `${showDate ? fromYmd(i.at.slice(0, 10)).toLocaleDateString('ru-RU', { day: 'numeric', month: 'short' }) + ', ' : ''}${hhmm(i.at)} · ${STATUS_TEXT[i.status]}`)),
      i.category_id ? h('span', { class: 'dot', style: `background:${catColor(i.category_id)}`, title: (catById(i.category_id) || {}).name }) : null,
      h('span', { 'aria-hidden': 'true', title: ALERT_TEXT[i.alert] }, ALERT_ICON[i.alert])));
}

async function dayBody() {
  const date = S.date;
  const [day, pending, stats] = await Promise.all([api('day', { date }), api('pending'), api('stats')]);
  const nodes = [];
  if (date === todayStr()) {
    const old = pending.filter((p) => p.at.slice(0, 10) < date);
    if (old.length) {
      nodes.push(h('div', { class: 'card alert-card' },
        h('div', { class: 'title' }, `Не отмечено: ${old.length}`),
        ...old.slice(0, 3).map((i) => itemRow(i, { showDate: true })),
        old.length > 3 ? h('div', { class: 'muted small', style: 'padding-top:6px' }, `и ещё ${old.length - 3} — откройте «Дела» или предыдущие дни`) : null));
    }
  }
  const total = day.items.length;
  const done = day.items.filter((i) => i.status === 'done_on_time' || i.status === 'done_late').length;
  if (total) {
    nodes.push(h('div', { class: 'card', style: 'padding:14px 16px' },
      h('div', { class: 'row' }, h('b', { class: 'grow' }, `Выполнено ${done} из ${total}`),
        stats.current_streak ? h('span', {}, `🔥 ${stats.current_streak} ${plural(stats.current_streak, ['день', 'дня', 'дней'])}`) : null),
      h('div', { class: 'progress-line', style: 'margin-top:10px' }, h('i', { style: `width:${Math.round((done / total) * 100)}%` }))));
  }
  nodes.push(h('div', { class: 'section-title' }, 'Дела'));
  nodes.push(total
    ? h('div', { class: 'card list' }, ...day.items.map((i) => itemRow(i)))
    : h('div', { class: 'card empty' }, h('div', { class: 'big' }, '🌤️'), h('b', {}, 'На этот день дел нет'), h('p', {}, 'Нажмите «+», чтобы добавить напоминание.')));
  nodes.push(h('div', { class: 'section-title' }, 'Расписание'));
  nodes.push(day.entries.length
    ? timetable(day.entries, date === todayStr())
    : h('div', { class: 'card empty' }, h('b', {}, 'Занятий нет'), h('p', {}, 'Добавьте занятие на этот день недели или на конкретную дату.'),
      h('button', { class: 'btn', onclick: () => scheduleSheet(null, { date }) }, 'Добавить занятие')));
  return h('div', { class: 'stack' }, nodes);
}

/* Раскладка пересекающихся занятий по дорожкам */
function layoutBlocks(entries) {
  const items = entries.map((e) => ({ e, s: toMin(e.start) / 60, f: toMin(e.end) / 60 })).sort((a, b) => a.s - b.s || a.f - b.f);
  const out = []; let cluster = []; let clusterEnd = -1;
  const flush = () => {
    const ends = [];
    cluster.forEach((it) => {
      let l = ends.findIndex((x) => x <= it.s);
      if (l < 0) { l = ends.length; ends.push(it.f); } else ends[l] = it.f;
      it.lane = l;
    });
    cluster.forEach((it) => { it.lanes = ends.length; });
    out.push(...cluster); cluster = [];
  };
  for (const it of items) {
    if (cluster.length && it.s >= clusterEnd) { flush(); clusterEnd = -1; }
    cluster.push(it); clusterEnd = Math.max(clusterEnd, it.f);
  }
  if (cluster.length) flush();
  return out;
}
function hourRange(entries, extraHours = []) {
  let lo = 8, hi = 20;
  entries.forEach((e) => { lo = Math.min(lo, Math.floor(toMin(e.start) / 60)); hi = Math.max(hi, Math.ceil(toMin(e.end) / 60)); });
  extraHours.forEach((x) => { lo = Math.min(lo, Math.floor(x)); hi = Math.max(hi, Math.ceil(x + 0.5)); });
  return [Math.max(0, lo), Math.min(24, hi)];
}
function blockEl(b, lo, onclick) {
  const e = b.e;
  const color = e.color || catColor(e.category_id);
  return h('button', {
    class: 'block', style: `--lane:${b.lane};--lanes:${b.lanes};--top:${b.s - lo};--len:${b.f - b.s};--c:${color}`,
    onclick: () => onclick(e), title: `${e.title} ${e.start}–${e.end}`,
  }, h('b', {}, FeatureUI.recordIcon(e.icon), ' ', e.title), h('span', {}, `${e.start}–${e.end}${e.location ? ' · ' + e.location : ''}`));
}
function timetable(entries, showNow) {
  const [lo, hi] = hourRange(entries);
  const n = hi - lo;
  const hours = h('div', { class: 'tt-hours' }, ...Array.from({ length: n + 1 }, (_, i) =>
    h('span', { style: `top:calc(${i} * var(--hh))` }, `${pad(lo + i)}:00`)));
  const track = h('div', { class: 'tt-track' }, h('div', { class: 'tt-lanes' }, ...layoutBlocks(entries).map((b) => blockEl(b, lo, (e) => scheduleSheet(e)))));
  if (showNow) {
    const now = new Date(); const nh = now.getHours() + now.getMinutes() / 60;
    if (nh >= lo && nh <= hi) track.append(h('div', { class: 'nowline', style: `top:calc(${nh - lo} * var(--hh))` }));
  }
  return h('div', { class: 'card tt', style: `--n:${n}` }, hours, track);
}

async function weekBody() {
  const week = await api('week', { date: S.date });
  const all = week.days.flatMap((d) => d.entries);
  const pinHours = week.days.flatMap((d) => d.items.map((i) => toMin(hhmm(i.at)) / 60));
  const [lo, hi] = hourRange(all, pinHours);
  const n = hi - lo;
  const today = todayStr();
  const cells = [h('div', { class: 'dayhead' })];
  week.days.forEach((d, i) => cells.push(h('button', {
    class: `dayhead${d.date === today ? ' today' : ''}`, onclick: () => { S.date = d.date; S.view = 'day'; render(); },
    'aria-label': `Открыть ${longDate(fromYmd(d.date))}`,
  }, WD_SHORT[i], h('b', {}, fromYmd(d.date).getDate()))));
  cells.push(h('div', { class: 'hourcol' }, ...Array.from({ length: n + 1 }, (_, i) => h('span', { style: `top:calc(${i} * var(--hh))` }, `${pad(lo + i)}`))));
  week.days.forEach((d) => {
    const col = h('div', { class: `col${d.date === today ? ' today' : ''}${d.items.length ? ' has-pins' : ''}` },
      h('div', { class: 'lanes' }, ...layoutBlocks(d.entries).map((b) => blockEl(b, lo, (e) => scheduleSheet(e)))));
    let lastY = -99;
    d.items.forEach((i) => {
      const nat = (toMin(hhmm(i.at)) / 60 - lo) * 46;
      const y = Math.max(nat, lastY + 26); lastY = y;
      const done = i.status === 'done_on_time' || i.status === 'done_late';
      col.append(h('button', {
        class: 'pin', 'data-status': i.status, style: `top:${y}px;--c:${catColor(i.category_id)}`,
        title: `${hhmm(i.at)} ${i.title}`, onclick: () => itemSheet(i),
      }, done ? '✓' : FeatureUI.recordIcon(i.icon)));
    });
    cells.push(col);
  });
  return h('div', { class: 'card tt week', style: `--n:${n}` }, h('div', { class: 'week-inner' }, cells));
}

async function itemSheet(i) {
  const reminder = (await api('reminders.list')).find(r => r.id === i.reminder_id);
  const done = i.status === 'done_on_time' || i.status === 'done_late';
  const close = openSheet(i.title, h('div', { class: 'stack' },
    h('div', { class: 'row' }, h('span', { class: 'ico', style: 'font-size:30px' }, FeatureUI.recordIcon(i.icon)),
      h('div', {}, h('div', {}, `${fromYmd(i.at.slice(0, 10)).toLocaleDateString('ru-RU', { day: 'numeric', month: 'long' })}, ${hhmm(i.at)}`),
        h('div', { class: 'muted' }, `${STATUS_TEXT[i.status]} · ${ALERT_TEXT[i.alert]}`))),
    i.notes ? h('p', { class: 'muted' }, i.notes) : null,
    FeatureUI.attachmentList(reminder?.attachments),
    h('div', { class: 'actions' },
      done ? h('button', { class: 'btn primary', onclick: () => { close(); doUndo(i); } }, 'Снять отметку')
        : h('button', { class: 'btn primary', onclick: () => { close(); doComplete(i); } }, 'Выполнено'),
      !done && i.status !== 'skipped' ? h('button', { class: 'btn', onclick: () => { close(); doSkip(i); } }, 'Пропустить') : null,
      !done && i.status !== 'skipped' ? h('button', {class: 'btn', onclick: () => { close(); FeatureUI.postpone(i); }}, 'Перенести') : null,
      i.status === 'skipped' ? h('button', { class: 'btn', onclick: () => { close(); doUndo(i); } }, 'Вернуть') : null,
      h('button', { class: 'btn', onclick: async () => { close(); reminderSheet((await api('reminders.list')).find((r) => r.id === i.reminder_id)); } }, 'Изменить'))));
}

// ---------- вкладка «Дела» ----------
function repeatText(r) {
  const t = r.start.slice(11, 16);
  switch (r.repeat.type) {
    case 'daily': return `Каждый день в ${t}`;
    case 'weekdays': return `${r.repeat.days.map((d) => WD_SHORT[d - 1]).join(', ')} в ${t}`;
    case 'monthly': return `Каждый месяц, ${Number(r.start.slice(8, 10))}-го в ${t}`;
    default: return `${fromYmd(r.start.slice(0, 10)).toLocaleDateString('ru-RU', { day: 'numeric', month: 'long', year: 'numeric' })}, ${t}`;
  }
}
async function viewReminders() {
  const all = await api('reminders.list', { archived: true });
  const shown = await FeatureUI.filterReminders(all.filter((r) => S.filterCat === null || r.category_id === S.filterCat));
  const chips = h('div', { class: 'chips scroll' },
    h('button', { class: 'chip', 'aria-pressed': String(S.filterCat === null), onclick: () => { S.filterCat = null; render(); } }, 'Все'),
    ...S.cats.map((c) => h('button', { class: 'chip', 'aria-pressed': String(S.filterCat === c.id), onclick: () => { S.filterCat = c.id; render(); } }, FeatureUI.recordIcon(c.icon), ' ', c.name)));
  const active = shown.filter((r) => !r.archived), arch = shown.filter((r) => r.archived);
  const row = (r) => h('button', { class: 'item', onclick: () => reminderSheet(r) },
    h('span', { class: 'ico' }, FeatureUI.recordIcon(r.icon)),
    h('span', { class: 'grow' }, h('span', { class: 'title', style: 'display:block' }, r.title), h('span', { class: 'muted small' }, repeatText(r))),
    r.category_id ? h('span', { class: 'dot', style: `background:${catColor(r.category_id)}` }) : null,
    h('span', { title: ALERT_TEXT[r.alert] }, ALERT_ICON[r.alert]));
  return h('div', { class: 'stack' }, FeatureUI.remindersSearch(), chips,
    active.length ? FeatureUI.groupRows(active, row)
      : h('div', { class: 'card empty' }, h('div', { class: 'big' }, '📝'), h('b', {}, 'Напоминаний пока нет'), h('p', {}, 'Создайте первое: укажите время, иконку и способ напоминания.'),
        h('button', { class: 'btn primary', onclick: () => reminderSheet(null) }, 'Создать напоминание')),
    arch.length ? [h('div', { class: 'section-title' }, 'В архиве'), h('div', { class: 'card list', style: 'opacity:.75' }, ...arch.map(row))] : null);
}

// ---------- редакторы ----------
function iconPicker(initial, onPick) { return FeatureUI.iconPicker(initial, onPick); }
function baseIconPicker(initial, onPick) {
  let value = initial;
  const input = h('input', { class: 'input', value: initial, maxlength: '8', 'aria-label': 'Иконка', style: 'text-align:center;font-size:22px;width:70px' });
  const grid = h('div', { class: 'emoji-grid' });
  const paint = () => {
    grid.replaceChildren(...EMOJI.map((e) => h('button', { type: 'button', 'aria-pressed': String(e === value), 'aria-label': e, onclick: () => { value = e; input.value = e; onPick(e); paint(); } }, e)));
  };
  input.addEventListener('input', () => { value = input.value; onPick(value); paint(); });
  paint();
  return h('div', { class: 'stack' }, h('div', { class: 'row' }, input, h('span', { class: 'muted small' }, 'Выберите или вставьте свой эмодзи')), grid);
}
const field = (label, control) => h('label', { class: 'field' }, label, control);
function segmented(options, value, onChange) {
  const wrap = h('div', { class: 'seg', role: 'radiogroup' });
  const paint = () => wrap.replaceChildren(...options.map(([v, l]) =>
    h('button', { type: 'button', role: 'radio', 'aria-checked': String(v === value), onclick: () => { value = v; onChange(v); paint(); } }, l)));
  paint();
  return wrap;
}
const GRACE = [[0, 'Точно в срок'], [15, '15 минут'], [30, '30 минут'], [60, '1 час'], [180, '3 часа'], [720, '12 часов'], [1440, 'Весь день']];

function reminderSheet(existing, preset = {}) {
  const now = new Date();
  const base = existing || {
    id: 0, title: '', notes: '', icon: '🔔', category_id: null, alert: 'notification', repeat: { type: 'none' },
    repeat_until: null, grace_minutes: 60, archived: false,
    start: `${preset.date || S.date}T${pad(Math.min(23, now.getHours() + 1))}:00:00`,
  };
  const r = JSON.parse(JSON.stringify(base));
  const attachments = FeatureUI.attachmentsEditor(r);
  let date = r.start.slice(0, 10), time = r.start.slice(11, 16);
  const title = h('input', { value: r.title, placeholder: 'Например, выпить таблетки', maxlength: '200', autofocus: true });
  const notes = h('textarea', { class: 'input', placeholder: 'Заметка (необязательно)', rows: '2' }, r.notes);
  notes.value = r.notes;
  const dateIn = h('input', { type: 'date', value: date, onchange: (e) => { date = e.target.value; } });
  const timeIn = h('input', { type: 'time', value: time, onchange: (e) => { time = e.target.value; } });
  const catSel = h('select', { onchange: (e) => { r.category_id = e.target.value ? Number(e.target.value) : null; } },
    h('option', { value: '' }, 'Без категории'),
    ...S.cats.map((c) => h('option', { value: c.id, selected: c.id === r.category_id }, c.name)));
  const graceSel = h('select', { onchange: (e) => { r.grace_minutes = Number(e.target.value); } },
    ...GRACE.map(([v, l]) => h('option', { value: v, selected: v === r.grace_minutes }, l)));
  if (!GRACE.some(([v]) => v === r.grace_minutes)) graceSel.prepend(h('option', { value: r.grace_minutes, selected: true }, `${r.grace_minutes} мин`));
  const untilIn = h('input', { type: 'date', value: r.repeat_until || '', onchange: (e) => { r.repeat_until = e.target.value || null; } });
  const repeatBox = h('div', { class: 'stack' });
  const paintRepeat = () => {
    const kids = [segmented([['none', 'Один раз'], ['daily', 'Каждый день'], ['weekdays', 'По дням'], ['monthly', 'Раз в месяц']], r.repeat.type, (v) => {
      r.repeat = v === 'weekdays' ? { type: 'weekdays', days: (r.repeat.days && r.repeat.days.length) ? r.repeat.days : [1, 2, 3, 4, 5] } : { type: v };
      paintRepeat();
    })];
    if (r.repeat.type === 'weekdays') {
      kids.push(h('div', { class: 'chips' }, ...WD_SHORT.map((l, i) => h('button', {
        type: 'button', class: 'chip', 'aria-pressed': String(r.repeat.days.includes(i + 1)),
        onclick: () => { const s = new Set(r.repeat.days); s.has(i + 1) ? s.delete(i + 1) : s.add(i + 1); r.repeat.days = [...s].sort(); paintRepeat(); },
      }, l))));
    }
    if (r.repeat.type !== 'none') kids.push(field('Повторять до (необязательно)', untilIn));
    repeatBox.replaceChildren(...kids);
  };
  paintRepeat();

  const save = async () => {
    r.title = title.value; r.notes = notes.value;
    if (!date || !time) { toast('Укажите дату и время', 'bad'); return; }
    r.start = `${date}T${time}:00`;
    if (r.repeat.type === 'none') r.repeat_until = null;
    try {
      await attachments.ready();
      await api('reminders.save', { reminder: r });
      close(); syncSoon(); toast(existing ? 'Сохранено' : 'Напоминание создано');
      if (!existing) permissionHint(r.alert);
      render();
    } catch (e) { toast(errText(e), 'bad'); }
  };
  const body = h('div', { class: 'stack' },
    iconPicker(r.icon, (v) => { r.icon = v || '🔔'; }),
    field('Название', title), notes, attachments,
    h('div', { class: 'two' }, field('Дата', dateIn), field('Точное время', timeIn)),
    h('div', { class: 'field' }, 'Как напомнить', segmented([['notification', '🔔 Уведомление'], ['alarm', '⏰ Будильник'], ['full_screen', '📱 На весь экран']], r.alert, (v) => { r.alert = v; })),
    h('div', { class: 'field' }, 'Повтор', repeatBox),
    h('div', { class: 'two' }, field('Категория', catSel), field('Засчитать «вовремя», если отметить в течение', graceSel)),
    h('div', { class: 'actions' },
      h('button', { class: 'btn primary', onclick: save }, 'Сохранить'),
      existing ? h('button', { class: 'btn', onclick: async () => { await api('reminders.archive', { id: r.id, archived: !r.archived }); close(); syncSoon(); render(); } }, r.archived ? 'Вернуть из архива' : 'В архив') : null,
      existing ? h('button', { class: 'btn danger', onclick: () => confirmSheet('Удалить напоминание? История выполнения и очки сохранятся.', 'Удалить', async () => { await api('reminders.delete', { id: r.id }); close(); syncSoon(); render(); }) }, 'Удалить') : null));
  const close = openSheet(existing ? 'Изменить напоминание' : 'Новое напоминание', body);
}

function scheduleSheet(existing, preset = {}) {
  const dateStr = preset.date || S.date;
  const wdOf = (s) => ((fromYmd(s).getDay() + 6) % 7) + 1;
  const e = existing ? JSON.parse(JSON.stringify(existing)) : {
    id: 0, title: '', weekday: wdOf(dateStr), date: null, start: '09:00', end: '10:30', color: null, icon: '📚', location: '', category_id: null,
  };
  let weekly = !e.date;
  const title = h('input', { value: e.title, placeholder: 'Например, лекция по физике', maxlength: '200' });
  const place = h('input', { value: e.location, placeholder: 'Аудитория, адрес или ссылка' });
  const startIn = h('input', { type: 'time', value: e.start, onchange: (x) => { e.start = x.target.value; } });
  const endIn = h('input', { type: 'time', value: e.end, onchange: (x) => { e.end = x.target.value; } });
  const wdSel = h('select', { onchange: (x) => { e.weekday = Number(x.target.value); } },
    ...WEEKDAYS.map((l, i) => h('option', { value: i + 1, selected: e.weekday === i + 1 }, l)));
  const dateIn = h('input', { type: 'date', value: e.date || dateStr, onchange: (x) => { e.date = x.target.value; } });
  const catSel = h('select', { onchange: (x) => { e.category_id = x.target.value ? Number(x.target.value) : null; } },
    h('option', { value: '' }, 'Без категории'),
    ...S.cats.map((c) => h('option', { value: c.id, selected: c.id === e.category_id }, c.name)));
  const colorIn = h('input', { type: 'color', value: e.color || catColor(e.category_id), onchange: (x) => { e.color = x.target.value; } });
  const whenBox = h('div', {});
  const paintWhen = () => whenBox.replaceChildren(weekly ? field('День недели', wdSel) : field('Дата', dateIn));
  paintWhen();
  const save = async () => {
    e.title = title.value; e.location = place.value; e.start = startIn.value; e.end = endIn.value;
    if (weekly) { e.date = null; e.weekday = Number(wdSel.value); } else { e.weekday = null; e.date = dateIn.value; }
    try { await api('schedule.save', { entry: e }); close(); syncSoon(); toast('Сохранено'); render(); } catch (err) { toast(errText(err), 'bad'); }
  };
  const body = h('div', { class: 'stack' },
    iconPicker(e.icon, (v) => { e.icon = v || '📚'; }),
    field('Название', title),
    segmented([['w', 'Каждую неделю'], ['d', 'В конкретный день']], weekly ? 'w' : 'd', (v) => { weekly = v === 'w'; paintWhen(); }),
    whenBox,
    h('div', { class: 'two' }, field('Начало', startIn), field('Конец', endIn)),
    field('Место', place),
    h('div', { class: 'two' }, field('Категория', catSel), field('Цвет', colorIn)),
    h('div', { class: 'actions' },
      h('button', { class: 'btn primary', onclick: save }, 'Сохранить'),
      existing ? h('button', { class: 'btn danger', onclick: () => confirmSheet('Удалить это занятие из расписания?', 'Удалить', async () => { await api('schedule.delete', { id: e.id }); close(); syncSoon(); render(); }) }, 'Удалить') : null));
  const close = openSheet(existing ? 'Изменить занятие' : 'Новое занятие', body);
}

function categorySheet(existing) {
  const c = existing ? { ...existing } : { id: 0, name: '', color: '#6750A4', icon: '📁' };
  const name = h('input', { value: c.name, placeholder: 'Название категории', maxlength: '40' });
  const color = h('input', { type: 'color', value: c.color, onchange: (x) => { c.color = x.target.value; } });
  const close = openSheet(existing ? 'Изменить категорию' : 'Новая категория', h('div', { class: 'stack' },
    iconPicker(c.icon, (v) => { c.icon = v || '📁'; }), field('Название', name), field('Цвет', color),
    h('div', { class: 'actions' },
      h('button', { class: 'btn primary', onclick: async () => { c.name = name.value; try { await api('categories.save', { category: c }); close(); render(); } catch (e) { toast(errText(e), 'bad'); } } }, 'Сохранить'),
      existing ? h('button', { class: 'btn danger', onclick: () => confirmSheet('Удалить категорию? Напоминания останутся без категории.', 'Удалить', async () => { await api('categories.delete', { id: c.id }); close(); render(); }) }, 'Удалить') : null)));
}

/* Подсказка про разрешения Android после создания будильника или полноэкранного напоминания */
async function permissionHint(alert) {
  if (IS_DEMO || alert === 'notification') return;
  const st = await native('status');
  if (st && (st.exactAlarms === false || st.fullScreen === false || st.notifications === false)) {
    toast('Для будильника нужны разрешения: Настройки → Android', 'gold');
  }
}

// ---------- вкладка «Успехи» ----------
async function viewProgress() {
  const [stats, ach] = await Promise.all([api('stats'), api('achievements')]);
  const hero = h('div', { class: 'card hero' },
    h('div', { class: 'ring', style: `--p:${Math.round(stats.level_progress * 100)}` }, h('div', {}, h('div', {}, h('b', {}, stats.level), h('span', {}, 'уровень')))),
    h('div', { class: 'grow' }, h('div', { style: 'font-size:22px;font-weight:700' }, `${stats.points} ${plural(stats.points, ['очко', 'очка', 'очков'])}`),
      h('div', { class: 'muted' }, `До следующего уровня ещё ${Math.max(0, Math.ceil(((stats.level) ** 2) * 50 - stats.points))}`)));
  const stat = (v, l) => h('div', { class: 'card stat' }, h('b', {}, v), h('span', { class: 'muted small' }, l));
  const grid = h('div', { class: 'statgrid' },
    stat(`🔥 ${stats.current_streak}`, 'дней подряд'), stat(stats.best_streak, 'лучшая серия'),
    stat(stats.total_done, 'выполнено'), stat(stats.on_time, 'вовремя'), stat(stats.missed, 'не выполнено'), stat(stats.skipped, 'пропущено'));
  const unlocked = ach.filter((a) => a.unlocked).length;
  const achGrid = h('div', { class: 'achgrid' }, ...ach.map((a) => h('div', { class: `card ach${a.unlocked ? '' : ' locked'}` },
    h('span', { class: 'ico' }, a.icon), h('b', {}, a.title), h('span', { class: 'muted small' }, a.description),
    a.unlocked ? h('span', { class: 'small', style: 'color:var(--ok)' }, `Получено · +${a.reward}`)
      : h('div', {}, h('div', { class: 'progress-line' }, h('i', { style: `width:${Math.round((a.progress / a.target) * 100)}%` })),
        h('span', { class: 'muted small' }, `${a.progress} из ${a.target} · награда ${a.reward}`)))));
  const boardBox = h('div', { class: 'stack' });
  const nodes = [hero, grid,
    h('div', { class: 'section-title' }, `Достижения: ${unlocked} из ${ach.length}`), achGrid,
    h('div', { class: 'section-title' }, 'Рейтинг'), boardBox];
  loadBoard(boardBox);
  return h('div', { class: 'stack' }, nodes);
}
async function loadBoard(box) {
  const s = S.settings;
  if (IS_DEMO) { box.replaceChildren(h('div', { class: 'card empty' }, h('p', {}, 'Рейтинг работает только в приложении.'))); return; }
  if (!(s['online.server'] || '').trim() || s['online.registered'] !== 'true') {
    box.replaceChildren(h('div', { class: 'card empty' }, h('div', { class: 'big' }, '🏆'), h('b', {}, 'Сравните себя с другими'),
      h('p', {}, 'Укажите адрес сервера и ник в настройках, чтобы попасть в рейтинг.'),
      h('button', { class: 'btn primary', onclick: () => { S.tab = 'settings'; render(); } }, 'Открыть настройки')));
    return;
  }
  const controls = h('div', { class: 'stack' },
    segmented([['week', 'Неделя'], ['all', 'Всё время']], S.board.period, (v) => { S.board.period = v; loadBoard(box); }),
    (s['online.room'] || '').trim() ? h('div', { class: 'chips' }, h('button', { class: 'chip', 'aria-pressed': String(S.board.roomOnly), onclick: () => { S.board.roomOnly = !S.board.roomOnly; loadBoard(box); } }, `Только комната «${s['online.room']}»`)) : null);
  box.replaceChildren(controls, h('div', { class: 'card empty muted' }, 'Загрузка…'));
  try {
    await api('online.push').catch(() => {});
    const res = await api('online.leaderboard', { period: S.board.period, room_only: S.board.roomOnly });
    const rows = res.entries.map((e) => h('div', { class: `item${e.me ? ' me' : ''}` },
      h('span', { class: 'rank' }, e.rank <= 3 ? ['🥇', '🥈', '🥉'][e.rank - 1] : e.rank),
      h('span', { class: 'grow' }, h('span', { class: 'title' }, e.nickname), h('span', { class: 'muted small', style: 'display:block' }, `уровень ${e.level} · 🔥 ${e.streak} · 🏅 ${e.achievements}`)),
      h('b', {}, e.points)));
    box.replaceChildren(controls, rows.length ? h('div', { class: 'card list board' }, ...rows) : h('div', { class: 'card empty muted' }, 'Пока никого нет'),
      res.me && res.me.rank > res.entries.length ? h('div', { class: 'card list board' }, h('div', { class: 'item me' }, h('span', { class: 'rank' }, res.me.rank), h('span', { class: 'grow title' }, 'Вы'), h('b', {}, res.me.points))) : null);
  } catch (e) {
    box.replaceChildren(controls, h('div', { class: 'card empty' }, h('p', {}, errText(e)), h('button', { class: 'btn', onclick: () => loadBoard(box) }, 'Повторить')));
  }
}

// ---------- вкладка «Настройки» ----------
async function viewSettings() {
  S.settings = await api('settings.get');
  const s = S.settings;
  const section = S.settingsSection || 'devices';
  const themeCards = h('div', { class: 'themes' }, ...THEMES.map(([id, name, desc]) => {
    const prevStyle = { material: 'background:color-mix(in srgb,var(--accent) 12%,#fff)', amoled: 'background:#000', glass: 'background:linear-gradient(135deg,var(--accent),#00bcd4)', simple: 'background:#f1f1f3', nord:'background:#2e3440', mono:'background:#242424' }[id];
    const bar = { material: 'background:var(--accent)', amoled: 'background:#1e1e22', glass: 'background:rgba(255,255,255,.45)', simple: 'background:#fff;border:1px solid #d8d8dd', nord:'background:#88c0d0', mono:'background:#a3a3a3' }[id];
    return h('button', { class: 'theme-card', 'aria-pressed': String((s.theme || 'material') === id), onclick: () => setSetting('theme', id, true) },
      h('div', { class: 'prev', style: prevStyle }, h('i', { style: `top:10px;${bar}` }), h('i', { style: `top:32px;width:60%;${bar}` })),
      h('b', {}, name), h('div', { class: 'muted small' }, desc));
  }));
  const accentBox = h('div', { class: 'swatches' }, ...ACCENTS.map((c) => h('button', { class: 'swatch', style: `--c:${c}`, 'aria-label': c, 'aria-pressed': String((s.accent || '').toLowerCase() === c.toLowerCase()), onclick: () => setSetting('accent', c, true) })),
    h('input', { type: 'color', value: s.accent || '#6750A4', 'aria-label': 'Свой цвет', style: 'width:38px;height:38px;border-radius:50%;padding:0;border:0', onchange: (e) => setSetting('accent', e.target.value.toUpperCase(), true) }));
  const bgChips = h('div', { class: 'chips' }, ...Background.modes.map(([id, name]) => h('button', { class: 'chip', 'aria-pressed': String((s.bg || 'none') === id), onclick: () => setSetting('bg', id, true) }, name)));
  const intensity = h('input', { type: 'range', min: '10', max: '100', value: s.bg_intensity || '60', 'aria-label': 'Яркость фона', oninput: (e) => { S.settings.bg_intensity = e.target.value; applyTheme(); }, onchange: (e) => setSetting('bg_intensity', e.target.value) });

  const catRows = S.cats.map((c) => h('button', { class: 'item', onclick: () => categorySheet(c) }, h('span', { class: 'ico' }, FeatureUI.recordIcon(c.icon)), h('span', { class: 'grow title' }, c.name), h('span', { class: 'dot', style: `background:${c.color}` })));

  const nodes = [
    IS_DEMO ? h('div', { class: 'card alert-card' }, h('b', {}, 'Демо-режим'), h('div', { class: 'muted small' }, 'Интерфейс открыт в браузере без Rust-ядра: данные хранятся только в этом браузере.')) : null,
    h('div', { class: 'section-title' }, 'Тема'), themeCards,
    h('div', { class: 'section-title' }, 'Светлая или тёмная'),
    segmented([['auto', 'Как в системе'], ['light', 'Светлая'], ['dark', 'Тёмная']], s.mode || 'auto', (v) => setSetting('mode', v)),
    h('div', { class: 'section-title' }, 'Акцентный цвет'), accentBox,
    h('div', {class: 'section-title'}, 'Иконки, шрифты и кнопки'), FeatureUI.customizationCard(),
    h('div', {class: 'section-title'}, 'Списки и безопасность'), FeatureUI.listCard(),
    h('div', { class: 'section-title' }, 'Анимированный фон'), bgChips, h('div', { style: 'margin-top:12px' }, field('Яркость', intensity)),
    h('div', { class: 'section-title' }, 'Категории'), h('div', { class: 'card list' }, ...catRows,
      h('button', { class: 'item', onclick: () => categorySheet(null) }, h('span', { class: 'ico' }, '+'), h('span', { class: 'grow title' }, 'Добавить категорию'))),
    h('div', { class: 'section-title' }, 'Рейтинг'), onlineCard(),
    h('div', { class: 'section-title' }, 'Синхронизация'), await syncCard(),
    h('div', {class: 'section-title'}, 'Локальные резервные копии'), FeatureUI.backupCard(),
  ];
  if (!IS_DEMO) {
    if (IS_ANDROID) nodes.push(h('div', {class: 'section-title'}, 'Android'), await androidCard());
    else { const desk = await desktopCard(); if (desk) nodes.push(h('div', { class: 'section-title' }, 'Компьютер'), desk); }
  }
  const tabs = h('div', {class:'settings-tabs',role:'tablist','aria-label':'Разделы настроек'},
    ...[['devices','Мои устройства'],['shared','Общие категории'],['appearance','Оформление'],['more','Дополнительно']].map(([id,label])=>h('button',{class:'chip',role:'tab','aria-selected':String(section===id),onclick:()=>{S.settingsSection=id;render();}},label)));
  const content = section==='devices' ? [Collaboration.devicesCard()]
    : section==='shared' ? [await Collaboration.sharedCard()]
    : section==='appearance' ? [h('div',{class:'section-title'},'Тема'),themeCards,segmented([['auto','Как в системе'],['light','Светлая'],['dark','Тёмная']],s.mode||'auto',v=>setSetting('mode',v)),accentBox,Collaboration.details('Иконки, шрифты и кнопки',FeatureUI.customizationCard()),Collaboration.details('Фон',bgChips,field('Яркость',intensity))]
    : [Collaboration.details('Списки и безопасность',FeatureUI.listCard()),Collaboration.details('Личные категории',h('div',{class:'card list'},...catRows,h('button',{class:'item',onclick:()=>categorySheet(null)},'Добавить категорию'))),Collaboration.details('Личная синхронизация вручную',await syncCard()),Collaboration.details('Локальные копии',FeatureUI.backupCard()),Collaboration.details('Рейтинг',onlineCard()),!IS_DEMO?(IS_ANDROID?await androidCard():await desktopCard()):null];
  return h('div',{class:'stack'},IS_DEMO?nodes[0]:null,tabs,...content);
}

// ---------- синхронизация между устройствами ----------
const fmtMs = (ms) => new Date(ms).toLocaleString('ru-RU', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
async function copyText(text) {
  try { await navigator.clipboard.writeText(text); toast('Скопировано'); }
  catch (_) { toast('Не удалось скопировать: выделите текст и скопируйте вручную', 'bad'); }
}
function downloadText(name, text) {
  FeatureUI.saveFile(name, new Blob([text], {type:'text/plain;charset=utf-8'})).catch(e => toast(errText(e),'bad'));
}

async function syncExportSheet() {
  let text;
  try { text = (await api('sync.export')).text; } catch (e) { toast(errText(e), 'bad'); return; }
  const area = h('textarea', { class: 'input', rows: '7', readonly: true, 'aria-label': 'Данные синхронизации', style: 'font-family:monospace;font-size:12px' }, text);
  openSheet('Файл синхронизации', h('div', { class: 'stack' },
    h('p', { class: 'muted small' }, 'Перенесите этот файл на другое устройство (мессенджер, облако, флешка, Bluetooth) и откройте там «Загрузить файл». Если вы уже в группе, файл зашифрован её кодом.'),
    area,
    h('div', { class: 'actions' },
      h('button', { class: 'btn primary', onclick: () => downloadText(`ежедневник-${todayStr()}.dpsync`, text) }, 'Скачать файл'),
      h('button', { class: 'btn', onclick: () => copyText(text) }, 'Копировать текст'))));
}

function syncImportSheet() {
  const area = h('textarea', { class: 'input', rows: '6', placeholder: 'Вставьте сюда текст файла синхронизации', 'aria-label': 'Данные синхронизации', style: 'font-family:monospace;font-size:12px' });
  const pick = h('input', { type: 'file', accept: '.dpsync,.txt,.json,text/plain,application/json', 'aria-label': 'Выбрать файл',
    onchange: async (e) => { const f = e.target.files && e.target.files[0]; if (f) { try { area.value = await f.text(); } catch (_) { toast('Не удалось прочитать файл', 'bad'); } } } });
  const close = openSheet('Загрузить файл синхронизации', h('div', { class: 'stack' },
    h('p', { class: 'muted small' }, 'Данные из файла сольются с вашими: побеждает более свежая правка каждой записи, ничего не пропадёт.'),
    pick, area,
    h('div', { class: 'actions' },
      h('button', { class: 'btn primary', onclick: async () => {
        if (!area.value.trim()) { toast('Выберите файл или вставьте текст', 'bad'); return; }
        try {
          const r = await api('sync.import', { text: area.value });
          close(); syncSoon(); pushScoreSoon();
          toast(r.applied ? `Получено изменений: ${r.applied}` : 'Всё уже актуально'); render();
        } catch (e) { toast(errText(e), 'bad'); }
      } }, 'Слить данные'))));
}

async function syncCard() {
  let st;
  try { st = await api('sync.status'); } catch (e) { return h('div', { class: 'card empty' }, h('p', {}, errText(e))); }
  const after = () => { syncSoon(); render(); };
  const nodes = [];

  if (!st.enabled) {
    const codeIn = h('input', { placeholder: 'ABCD-EFGH-JKLM-NPQR', autocapitalize: 'characters', autocomplete: 'off', spellcheck: 'false', maxlength: '24', 'aria-label': 'Код группы', style: 'font-family:monospace;letter-spacing:1px' });
    nodes.push(
      h('p', { class: 'muted small' }, 'Телефон и компьютер можно связать без своего сервера. На одном устройстве создайте группу, на другом введите её код: данные будут обмениваться через интернет или по Wi-Fi, всё зашифровано этим кодом.'),
      h('div', { class: 'actions', style: 'margin-top:0' },
        h('button', { class: 'btn primary', onclick: async () => {
          try { await api('sync.create'); after(); } catch (e) { toast(errText(e), 'bad'); }
        } }, 'Создать группу')),
      field('У меня уже есть код', codeIn),
      h('div', { class: 'actions', style: 'margin-top:0' },
        h('button', { class: 'btn', onclick: async () => {
          try { await api('sync.join', { code: codeIn.value }); toast('Вы в группе. Данные придут в течение минуты'); after(); } catch (e) { toast(errText(e), 'bad'); }
        } }, 'Присоединиться')));
  } else {
    const cfg = async (patch) => {
      try { await api('sync.config', { ntfy: st.ntfy, lan: st.lan, server: st.server, ...patch }); render(); } catch (e) { toast(errText(e), 'bad'); }
    };
    const serverIn = h('input', { value: st.server, inputmode: 'url', autocapitalize: 'off', 'aria-label': 'Сервер ntfy', onchange: (e) => cfg({ server: e.target.value }) });
    const addrIn = h('input', { placeholder: '192.168.1.5', inputmode: 'url', autocapitalize: 'off', 'aria-label': 'Адрес устройства' });
    const runBtn = h('button', { class: 'btn primary' }, 'Синхронизировать сейчас');
    runBtn.onclick = async () => {
      runBtn.disabled = true; runBtn.textContent = 'Синхронизация…';
      try {
        const r = await api('sync.run');
        toast(r.applied ? `Получено изменений: ${r.applied}` : 'Всё актуально'); after();
      } catch (e) { toast(errText(e), 'bad'); render(); }
    };
    nodes.push(
      h('div', { class: 'field' }, 'Код вашей группы',
        h('div', { class: 'row' }, h('div', { class: 'sync-code', 'aria-label': 'Код группы' }, st.code),
          h('button', { class: 'btn small', onclick: () => copyText(st.code) }, 'Копировать'))),
      h('p', { class: 'muted small' }, 'Введите этот код на других устройствах. Кто знает код, тот может читать данные группы: храните его в тайне.'),
      h('div', { class: 'muted small' }, st.last_ok ? `Последняя синхронизация: ${fmtMs(st.last_ok)}` : 'Синхронизации ещё не было'),
      st.error ? h('div', { class: 'card alert-card' }, h('div', { class: 'small' }, st.error)) : null,
      h('div', { class: 'chips' },
        h('button', { class: 'chip', 'aria-pressed': String(st.ntfy), onclick: () => cfg({ ntfy: !st.ntfy }) }, 'Через интернет'),
        h('button', { class: 'chip', 'aria-pressed': String(st.lan), onclick: () => cfg({ lan: !st.lan }) }, 'По локальной сети')),
      h('div', { class: 'actions', style: 'margin-top:0' }, runBtn),
      h('details', { class: 'sync-more' },
        h('summary', {}, 'Дополнительно'),
        h('div', { class: 'stack', style: 'padding-top:10px' },
          field('Сервер ntfy (по умолчанию бесплатный ntfy.sh, можно указать свой)', serverIn),
          st.lan_addr ? h('p', { class: 'muted small' }, `Адрес этого устройства в сети: ${st.lan_addr}. Устройства обычно находят друг друга сами; если нет, введите адрес другого устройства здесь.`) : null,
          field('Подключиться к устройству по адресу', addrIn),
          h('div', { class: 'actions', style: 'margin-top:0' },
            h('button', { class: 'btn small', onclick: async () => {
              try { const r = await api('sync.lan_connect', { addr: addrIn.value }); toast(r.applied ? `Получено изменений: ${r.applied}` : 'Всё актуально'); after(); } catch (e) { toast(errText(e), 'bad'); }
            } }, 'Подключиться')),
          h('button', { class: 'btn danger small', onclick: () => confirmSheet('Выйти из группы? Данные на этом устройстве останутся, но обмен с группой прекратится.', 'Выйти', async () => {
            try { await api('sync.leave'); render(); } catch (e) { toast(errText(e), 'bad'); }
          }) }, 'Выйти из группы'))));
  }

  nodes.push(
    h('div', { class: 'section-title', style: 'margin:6px 0 0' }, 'Через файл'),
    h('div', { class: 'row', style: 'flex-wrap:wrap' },
      h('button', { class: 'btn small', onclick: syncExportSheet }, 'Сохранить файл'),
      h('button', { class: 'btn small', onclick: syncImportSheet }, 'Загрузить файл')));
  return h('div', { class: 'card', style: 'padding:14px' }, h('div', { class: 'stack' }, nodes));
}

function onlineCard() {
  const s = S.settings;
  const server = h('input', { value: s['online.server'] || '', placeholder: 'https://example.com:8787', inputmode: 'url', autocapitalize: 'off' });
  const nick = h('input', { value: s['online.nickname'] || '', placeholder: 'Ваш ник', maxlength: '24' });
  const room = h('input', { value: s['online.room'] || '', placeholder: 'Например, друзья (необязательно)', maxlength: '32' });
  const registered = s['online.registered'] === 'true';
  return h('div', { class: 'card', style: 'padding:14px' }, h('div', { class: 'stack' },
    field('Адрес сервера рейтинга', server), field('Ник', nick), field('Комната (общий рейтинг для своих)', room),
    h('div', { class: 'actions', style: 'margin-top:0' },
      h('button', { class: 'btn primary', onclick: async () => {
        try {
          await api('settings.set', { key: 'online.server', value: server.value.trim() });
          await api('settings.set', { key: 'online.room', value: room.value.trim() });
          if (!registered) await api('online.register', { nickname: nick.value });
          await api('online.push');
          toast(registered ? 'Сохранено' : 'Вы в рейтинге'); render();
        } catch (e) { toast(errText(e), 'bad'); }
      } }, registered ? 'Сохранить и отправить очки' : 'Войти в рейтинг'),
      registered ? h('button', { class: 'btn', onclick: async () => { await api('online.logout'); render(); } }, 'Выйти') : null),
    h('p', { class: 'muted small' }, 'Сервер находится в папке server проекта. Очки считаются на вашем устройстве и отправляются на сервер.')));
}

async function desktopCard() {
  let st = null;
  try { st = await TAURI.core.invoke('desktop_status'); } catch (_) { return null; }
  if (!st) return null;
  const autostart = h('button', {
    class: 'chip', 'aria-pressed': String(!!st.autostart),
    onclick: async () => {
      try { await TAURI.core.invoke('desktop_set_autostart', { enabled: !st.autostart }); } catch (e) { toast(errText(e), 'bad'); }
      render();
    },
  }, 'Запускать вместе с Windows');
  const test = (kind, label) => h('button', {
    class: 'btn small',
    onclick: () => TAURI.core.invoke('desktop_test', { kind }).catch((e) => toast(errText(e), 'bad')),
  }, label);
  return h('div', { class: 'card', style: 'padding:14px' }, h('div', { class: 'stack' },
    h('p', { class: 'muted small' }, 'Приложение работает в трее: закрытие окна сворачивает его, и напоминания продолжают срабатывать. Выйти можно через меню значка в трее.'),
    h('div', { class: 'chips' }, autostart),
    h('div', { class: 'row', style: 'flex-wrap:wrap' },
      test('notification', 'Проверить уведомление'), test('alarm', 'Проверить будильник'), test('full_screen', 'Проверить на весь экран'))));
}

// ---------- запуск ----------
async function boot() {
  await updatePlatformInsets();
  await FeatureUI.unlockGate();
  try { S.settings = await api('settings.get'); } catch (_) { S.settings = {}; }
  await FeatureUI.init();
  S.filterCat = S.settings['list.default_category'] ? Number(S.settings['list.default_category']) : null;
  applyTheme();
  await render();
  if (!IS_DEMO) {
    await drainNativeActions();
    syncNative();
    document.addEventListener('visibilitychange', async () => {
      if (document.hidden) return;
      updatePlatformInsets();
      const changed = await drainNativeActions();
      syncSoon();
      if (changed || S.tab === 'today') render();
    });
    setInterval(() => { if (!document.hidden && S.tab === 'today') render(); }, 60000);
    // Отметка в окне напоминания: перерисовать список (не мешая открытому редактору).
    if (TAURI.event && TAURI.event.listen) {
      TAURI.event.listen('planner-changed', () => { syncSoon(); if (!$('.sheet')) render(); pushScoreSoon(); });
    }
  } else {
    setInterval(() => { if (!document.hidden && S.tab === 'today' && !$('.sheet')) render(); }, 60000);
  }
}
boot().catch(e => { $('#view').replaceChildren(h('div', {class:'card empty'}, h('p',{},errText(e)))); });

async function androidCard() {
  const st=await native('status');
  if (!st) return h('div',{class:'card empty muted small'},'Настройки Android доступны только на телефоне.');
  const rows=[['notifications','Уведомления','Нужны для напоминаний'],['exactAlarms','Точные будильники','Срабатывание в назначенную минуту'],['fullScreen','На весь экран','Показывать поверх блокировки'],['batteryOk','Работа в фоне','Исключить приложение из экономии батареи']];
  return h('div',{class:'card list'},...rows.map(([key,title,desc])=>h('div',{class:'item'},h('span',{class:'ico'},st[key]?'✅':'⚠️'),h('span',{class:'grow'},h('b',{},title),h('span',{class:'muted small',style:'display:block'},desc)),st[key]?null:h('button',{class:'btn small',onclick:async()=>{await native('request',{kind:key==='batteryOk'?'battery':key==='fullScreen'?'fullScreen':key});setTimeout(render,800);}},'Разрешить'))));
}
