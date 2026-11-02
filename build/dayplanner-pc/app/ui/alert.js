'use strict';
/* Окно напоминания (alert.html). Данные берёт у Rust командой `alert_info`,
   кнопки вызывают `alert_action`. Виды: notification, alarm, full_screen. */

const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);

let closing = false;

function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === null || v === undefined || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k === 'style') el.style.cssText = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else el.setAttribute(k, v);
  }
  kids.flat().forEach((c) => { if (c !== null && c !== undefined && c !== false) el.append(c.nodeType ? c : document.createTextNode(String(c))); });
  return el;
}

/* Звук играет Rust (системный сигнал Windows). Здесь только кнопка «Выключить звук». */
function muteSound() {
  const btn = document.getElementById('sound');
  if (btn) btn.style.display = 'none';
  invoke('alert_mute').catch(() => {});
}

/* ----- действия ----- */
function showError(text) {
  let box = document.getElementById('err');
  if (!box) {
    box = h('div', { id: 'err', class: 'err', role: 'alert' });
    document.body.append(box);
  }
  box.textContent = text;
}

async function act(action) {
  if (closing) return;
  closing = true;
  try {
    await invoke('alert_action', { action });
  } catch (e) {
    // Окно остаётся открытым: пользователь видит, что отметка не записалась.
    closing = false;
    showError(typeof e === 'string' ? e : 'Не удалось выполнить действие');
  }
}

function hex(accent) { return /^#[0-9a-f]{6}$/i.test(accent || '') ? accent : '#6750A4'; }
function luminance(c) {
  const n = parseInt(c.slice(1), 16);
  const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
  return 0.2126 * f((n >> 16) & 255) + 0.7152 * f((n >> 8) & 255) + 0.0722 * f(n & 255);
}

async function main() {
  const p = await invoke('alert_info');
  if (!p) { act('close'); return; }

  const accent = hex(p.accent);
  document.documentElement.style.setProperty('--accent', accent);
  // Цвет текста на белой кнопке: тёмная версия акцента, чтобы читался в любой теме.
  document.documentElement.style.setProperty('--accent-dark', luminance(accent) > 0.5 ? '#2b2b2b' : `color-mix(in srgb, ${accent} 70%, #000)`);

  const kind = ['alarm', 'full_screen'].includes(p.alert) ? p.alert : 'notification';
  const time = (p.at || '').slice(11, 16);

  const btn = (label, action, primary) => h('button', { class: `btn${primary ? ' primary' : ''}`, onclick: () => act(action) }, label);

  const info = [
    h('div', { class: `icon${kind === 'alarm' ? ' pulse' : ''}` }, /^data:image\/png;base64,[A-Za-z0-9+/=]+$/.test(p.icon || '') ? h('img',{src:p.icon,alt:'',style:'width:64px;height:64px;object-fit:contain'}) : p.icon || '🔔'),
    h('div', {},
      h('div', { class: 'title' }, p.title || ''),
      h('div', { class: 'time' }, time ? `Назначено на ${time}` : ''),
      p.notes ? h('div', { class: 'notes' }, p.notes) : null),
  ];

  const root = document.getElementById('root');
  if (kind === 'notification') {
    root.append(h('div', { class: 'pop notification', role: 'alertdialog', 'aria-label': p.title },
      h('button', { class: 'close', 'aria-label': 'Закрыть', onclick: () => act('close') }, '✕'),
      h('div', { class: 'head' }, ...info),
      h('div', { class: 'actions' }, btn('Выполнено', 'done', true), btn('Пропустить', 'skip'), btn('Через 10 мин', 'snooze'))));
    // Уведомление исчезает само, а дело остаётся в списке как «пора выполнить».
    setTimeout(() => act('close'), 45000);
  } else {
    const actions = [btn('Выполнено', 'done', true)];
    if (kind === 'alarm') actions.push(h('button', { id: 'sound', class: 'btn', style: 'display:inline-block', onclick: muteSound }, 'Выключить звук'));
    actions.push(btn('Через 10 минут', 'snooze'), btn('Пропустить', 'skip'));
    root.append(h('div', { class: `pop ${kind}`, role: 'alertdialog', 'aria-label': p.title },
      ...info.flatMap((n) => (n.classList.contains('icon') ? [n] : [...n.children])),
      h('div', { class: 'actions' }, ...actions)));
  }

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') { if (kind === 'notification') act('close'); else if (kind === 'alarm') muteSound(); }
  });
}

main().catch(() => { try { invoke('alert_action', { action: 'close' }); } catch (_) { /* окно закроет пользователь */ } });
