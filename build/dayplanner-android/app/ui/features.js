'use strict';
/* Additions inspired by To-do-list, reimplemented for the Rust/Tauri API. */
const FeatureUI = (() => {
  let icons = {}, idleTimer = 0, idleDeadline = 0, lockEnabled = false;
  const FONT = { system: null, sans: '"PT Sans", sans-serif', serif: '"PT Serif", serif', ubuntu: 'Ubuntu, sans-serif', mono: '"Roboto Mono", monospace' };
  const PACKS = [['default', 'Контурные'], ['rounded', 'Мягкие'], ['minimalistic', 'Минималистичные'], ['minimalistic-blue', 'Синие'], ['old', 'Классические'], ['custom', 'Мой ZIP-набор']];
  const FALLBACK = { plusS: '+', settings: '⚙', nazad: '‹', update: '↻', basket1: '×', redact: '✎', info: 'ⓘ', auth: '☁', logout: '↪', undo: '↶' };
  const isImage = value => typeof value === 'string' && /^data:image\/png;base64,[A-Za-z0-9+/=]+$/.test(value);
  const RECORDS = {
    task: ['Дело', 'M5 12l4 4L19 6 M4 3h16v18H4z'],
    note: ['Заметка', 'M5 3h10l4 4v14H5z M14 3v5h5 M8 12h8 M8 16h6'],
    work: ['Работа', 'M3 7h18v14H3z M8 7V3h8v4 M3 12h18 M10 12v3h4v-3'],
    home: ['Дом', 'M3 11l9-8 9 8 M5 10v11h14V10 M9 21v-7h6v7'],
    study: ['Учёба', 'M3 4h7l2 2 2-2h7v15h-7l-2 2-2-2H3z M12 6v15'],
    heart: ['Здоровье', 'M12 21L3 12C-2 4 8 0 12 7c4-7 14-3 9 5z'],
    sport: ['Спорт', 'M3 8v8 M6 5v14 M6 12h12 M18 5v14 M21 8v8'],
    cart: ['Покупки', 'M2 3h3l3 13h11l3-9H6 M9 21h1 M18 21h1'],
    calendar: ['Дата', 'M3 5h18v16H3z M7 2v6 M17 2v6 M3 10h18 M7 14h3 M14 14h3'],
    clock: ['Время', 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18 M12 7v5l4 2'],
    star: ['Важное', 'M12 2l3 7 8 1-6 5 2 8-7-4-7 4 2-8-6-5 8-1z'],
    bell: ['Напоминание', 'M5 16h14l-2-4V8a5 5 0 0 0-10 0v4z M10 20h4'],
    code: ['Разработка', 'M8 6l-6 6 6 6 M16 6l6 6-6 6 M14 3l-4 18'],
    music: ['Музыка', 'M9 18V5l11-2v13 M9 9l11-2 M9 18c0 4-6 4-6 0s6-4 6 0 M20 16c0 4-6 4-6 0s6-4 6 0'],
    travel: ['Поездка', 'M2 13l8-3V3l2-1 2 1v7l8 3v2l-8-1v5l3 2v1l-5-1-5 1v-1l3-2v-5l-8 1z'],
    folder: ['Категория', 'M2 6h8l2 3h10v12H2z M2 6V3h7l3 3h8v3']
  };
  const LEGACY = {'🔔':'bell','📝':'note','📌':'note','💼':'work','🏠':'home','📚':'study','🎓':'study','❤️':'heart','💊':'heart','🏃':'sport','💪':'sport','🛒':'cart','📅':'calendar','⏰':'clock','⭐':'star','💻':'code','🎵':'music','✈️':'travel','📁':'folder'};
  const fileBase64 = file => new Promise((resolve, reject) => {
    const reader = new FileReader(); reader.onload = () => resolve(String(reader.result).split(',')[1]); reader.onerror = () => reject(new Error('Не удалось прочитать файл')); reader.readAsDataURL(file);
  });
  const safe = async action => { try { return await action(); } catch (e) { toast(errText(e), 'bad'); } };
  const nativeRequest = (cmd, args) => TAURI.core.invoke('plugin:planner-native|native', {cmd, args});
  function icon(name, fallback) {
    const pack = S.settings.icon_pack || 'default';
    const scheme = document.documentElement.dataset.scheme || 'light';
    let src = pack === 'custom' && icons[name] && isImage(icons[name]) ? icons[name] : null;
    if (['minimalistic', 'minimalistic-blue', 'old'].includes(pack) && Object.hasOwn(FALLBACK, name) && name !== 'undo') {
      const vector = ['basket1', 'redact'].includes(name) && pack !== 'old';
      src = `assets/icons/${pack}/${scheme}/${vector ? 'svg/' : ''}${name}.${vector ? 'svg' : 'png'}`;
    }
    if (src) return h('img', {class: 'ui-icon', src, alt: '', 'aria-hidden': 'true', onerror: e => { e.target.replaceWith(fallback?.nodeType ? fallback : document.createTextNode(fallback || FALLBACK[name] || '•')); }});
    if (pack === 'rounded' && fallback?.setAttribute) fallback.style.strokeWidth = '2.4';
    return fallback || h('span', {class: 'action-symbol', 'aria-hidden': 'true'}, FALLBACK[name] || '•');
  }
  function recordIcon(value) {
    if (isImage(value)) return h('img', {src: value, class: 'record-icon', alt: ''});
    const key = String(value || '').replace(/^builtin:/, '');
    const [label, path] = RECORDS[LEGACY[value] || key] || RECORDS.task;
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    for (const [k,v] of Object.entries({viewBox:'0 0 24 24',fill:'none',stroke:'currentColor','stroke-width':'1.7','stroke-linecap':'round','stroke-linejoin':'round',class:'record-vector','aria-hidden':'true'})) svg.setAttribute(k,v);
    const p = document.createElementNS(svg.namespaceURI, 'path'); p.setAttribute('d', path); svg.append(p);
    return svg;
  }
  function apply(settings) {
    const root = document.documentElement;
    const font = FONT[settings.font_family || 'system'];
    if (font) root.style.setProperty('--font', font); else root.style.removeProperty('--font');
    root.style.setProperty('--font-scale', ['0.9', '1', '1.1', '1.2'].includes(settings.font_scale) ? settings.font_scale : '1');
    const color = /^#[0-9a-f]{6}$/i.test(settings.button_color || '') ? settings.button_color : 'var(--accent)';
    root.style.setProperty('--button-accent', color);
    root.style.setProperty('--on-button', /^#[0-9a-f]{6}$/i.test(settings.button_color || '') ? luminance(settings.button_color) > .4 ? '#161616' : '#ffffff' : 'var(--on-accent)');
    root.dataset.iconPack = settings.icon_pack || 'default';
  }
  async function init() { icons = await api('icons.get').catch(() => ({})); lockEnabled = !!(await api('security.status')).enabled; apply(S.settings); resetIdle(); }
  async function unlockGate() {
    const state = await api('security.status');
    if (!state.locked) return true;
    return new Promise(resolve => {
      $('#nav').hidden = true;
      $('#top').replaceChildren(h('h1', {}, 'Ежедневник'));
      const password = h('input', {type: 'password', class: 'input', autocomplete: 'current-password', 'aria-label': 'Пароль приложения', placeholder: 'Пароль приложения'});
      const error = h('p', {class: 'bad-text', role: 'alert'});
      const submit = h('button', {type: 'submit', class: 'btn primary'}, 'Открыть');
      const form = h('form', {class: 'card lock-card', onsubmit: async e => {
        e.preventDefault(); submit.disabled = true;
        try { await api('security.unlock', {password: password.value}); password.value = ''; $('#nav').hidden = false; resolve(true); }
        catch (e) { password.value = ''; error.textContent = errText(e); }
        finally { submit.disabled = false; }
      }}, h('div', {class: 'big'}, '🔒'), h('h2', {}, 'Приложение заблокировано'), password, error, submit);
      $('#view').replaceChildren(form); password.focus();
    });
  }
  function resetIdle(event) {
    if (idleDeadline && Date.now() >= idleDeadline && lockEnabled) {
      event?.preventDefault(); event?.stopImmediatePropagation(); safe(lockNow); return;
    }
    clearTimeout(idleTimer);
    if (IS_DEMO) return;
    const minutes = Number(S.settings['list.lock_minutes'] || 5);
    idleDeadline = minutes > 0 ? Date.now() + minutes * 60000 : 0;
    if (minutes > 0 && lockEnabled) idleTimer = setTimeout(() => safe(lockNow), minutes * 60000);
  }
  async function lockNow() { await api('security.lock'); location.reload(); }
  document.addEventListener('pointerdown', resetIdle, {capture: true});
  document.addEventListener('keydown', resetIdle, {capture: true});
  document.addEventListener('visibilitychange', () => { if (!document.hidden && idleDeadline && Date.now() >= idleDeadline && lockEnabled) safe(lockNow); });
  document.addEventListener('keydown', e => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z' && !e.shiftKey && !e.target.closest('input,textarea,[contenteditable=true]') && !document.querySelector('.sheet')) {
      e.preventDefault(); safe(async () => { if ((await api('history.status')).available) { await api('history.undo'); syncSoon(); render(); } });
    }
  });

  async function saveFile(name, blob) {
    if (!IS_DEMO) {
      const result = await nativeRequest('save_file', {name, data: await fileBase64(blob)});
      if (result?.saved) toast(result.path ? `Сохранено: ${result.path}` : 'Файл сохранён');
      return;
    }
    const url = URL.createObjectURL(blob), link = h('a', {href: url, download: name});
    document.body.append(link); link.click(); link.remove(); setTimeout(() => URL.revokeObjectURL(url), 5000);
  }
  function attachmentBlob(file) {
    const bytes = Uint8Array.from(atob(file.data), c => c.charCodeAt(0));
    return new Blob([bytes], {type: file.mime || 'application/octet-stream'});
  }
  function attachmentViewer(file) {
    const image = /^image\/(png|jpeg|webp|gif)$/.test(file.mime);
    let zoom = 1, angle = 0;
    const img = image ? h('img', {src: `data:${file.mime};base64,${file.data}`, alt: file.name}) : null;
    const stage = image ? h('div', {class: 'image-stage'}, img) : h('p', {class: 'muted'}, 'Этот тип файла можно скачать. Автоматический запуск отключён.');
    const paint = () => { img.style.width = `${zoom * 100}%`; img.style.transform = `rotate(${angle}deg)`; };
    openSheet(file.name, h('div', {class: 'stack'}, stage,
      image ? h('div', {class: 'chips'}, ...[['−', () => zoom = Math.max(.25, zoom - .25)], ['+', () => zoom = Math.min(4, zoom + .25)], ['Повернуть', () => angle = (angle + 90) % 360]].map(([label, change]) => h('button', {class: 'chip', onclick: () => { change(); paint(); }}, label))) : null,
      h('button', {class: 'btn primary', onclick: () => safe(() => saveFile(file.name, attachmentBlob(file)))}, 'Скачать исходный файл')));
  }
  function attachmentList(files) {
    return h('div', {class: 'attachment-list'}, ...(files || []).map(file => h('button', {type: 'button', class: 'chip attachment-chip', onclick: () => attachmentViewer(file)}, '📎 ', file.name)));
  }
  function attachmentsEditor(record) {
    record.attachments = (record.attachments || []).map(f => ({...f}));
    const list = h('div', {class: 'attachment-list'}), status = h('p', {class: 'muted small', role: 'status'});
    let pending = Promise.resolve();
    const add = files => {
      pending = pending.then(async () => {
        for (const file of files) {
          try {
            if (file.size > 10 * 1024 * 1024 || record.attachments.length >= 10) throw 'Лимит: 10 МБ на файл и 10 файлов на запись';
            const used = record.attachments.reduce((sum, f) => sum + Math.floor(f.data.length * .75), 0);
            if (used + file.size > 20 * 1024 * 1024) throw 'Вложения записи больше 20 МБ';
            status.textContent = 'Добавление…';
            const attachment = await api('attachments.prepare', {name: file.name, mime: file.type || 'application/octet-stream', data: await fileBase64(file)});
            if (!record.attachments.some(f => f.id === attachment.id)) record.attachments.push(attachment);
            paint();
          } catch (e) { toast(errText(e), 'bad'); }
        }
        status.textContent = '';
      });
    };
    const pick = h('input', {type: 'file', multiple: true, 'aria-label': 'Добавить вложения', onchange: e => { add([...e.target.files]); e.target.value = ''; }});
    const paint = () => list.replaceChildren(...record.attachments.map(file => h('div', {class: 'row'}, h('button', {type: 'button', class: 'chip grow attachment-chip', onclick: () => attachmentViewer(file)}, '📎 ', file.name), h('button', {type: 'button', class: 'icon-btn', 'aria-label': `Убрать ${file.name}`, onclick: () => { record.attachments = record.attachments.filter(f => f !== file); paint(); }}, icon('basket1')))));
    const root = h('div', {class: 'attachments-editor stack', ondragover: e => e.preventDefault(), ondrop: e => { e.preventDefault(); add([...e.dataTransfer.files]); }}, h('b', {}, 'Вложения'), pick, list, status, h('p', {class: 'muted small'}, 'До 10 МБ на файл, 20 МБ на запись. Можно перетащить файлы или вставить изображение из буфера.'));
    root.addEventListener('paste', e => { if (e.clipboardData?.files.length) { e.preventDefault(); add([...e.clipboardData.files]); } });
    root.ready = () => pending;
    paint(); return root;
  }
  function passwordSheet() {
    const current = h('input', {type: 'password', class: 'input', autocomplete: 'current-password'});
    const password = h('input', {type: 'password', class: 'input', autocomplete: 'new-password', placeholder: 'Минимум 8 символов'});
    const repeat = h('input', {type: 'password', class: 'input', autocomplete: 'new-password'});
    const close = openSheet('Пароль приложения', h('div', {class: 'stack'},
      h('p', {class: 'muted small'}, 'Пароль закрывает интерфейс и API. Это не шифрование всей SQLite-базы; системные напоминания остаются видимыми. Облачные снимки шифруются кодом группы.'),
      field('Текущий пароль (если установлен)', current), field('Новый пароль; пусто — снять защиту', password), field('Повторите новый пароль', repeat),
      h('button', {class: 'btn primary', onclick: () => safe(async () => { if (password.value !== repeat.value) throw 'Пароли не совпадают'; lockEnabled = (await api('security.password', {current: current.value, password: password.value})).enabled; current.value = password.value = repeat.value = ''; close(); resetIdle(); toast('Пароль обновлён'); render(); })}, 'Сохранить')));
  }
  function select(options, value, change) { return h('select', {onchange: e => change(e.target.value)}, ...options.map(([id, label]) => h('option', {value: id, selected: id === value}, label))); }
  function customizationCard() {
    const s = S.settings;
    const packFile = h('input', {type: 'file', accept: '.zip,application/zip', 'aria-label': 'Загрузить ZIP-набор иконок', onchange: e => safe(async () => {
      const file = e.target.files[0]; if (!file) return;
      if (file.size > 20 * 1024 * 1024) throw 'Архив больше 20 МБ';
      icons = await api('icons.import', {data: await fileBase64(file)}); await setSetting('icon_pack', 'custom', true); toast('Набор загружен');
    })});
    return h('div', {class: 'card settings-extra stack'},
      field('Иконки интерфейса', select(PACKS, s.icon_pack || 'default', v => setSetting('icon_pack', v, true))),
      h('div', {class: 'icon-preview'}, ...Object.keys(FALLBACK).map(name => h('span', {title: name}, icon(name)))),
      field('Свой набор', packFile),
      h('details', {}, h('summary', {}, 'Формат ZIP'), h('p', {class: 'muted small'}, 'PNG/JPEG/WebP с именами today, list, note, trophy, settings, plusS, auth, update, nazad, basket1, redact, info, logout, undo. Расширения соответствуют формату. Папка внутри ZIP допустима; отсутствующие иконки заменяются стандартными. До 20 МБ. SVG из пользовательских архивов не принимаются.')),
      field('Шрифт (работает без интернета)', select([['system', 'Системный'], ['sans', 'PT Sans'], ['serif', 'PT Serif'], ['ubuntu', 'Ubuntu'], ['mono', 'Roboto Mono']], s.font_family || 'system', v => setSetting('font_family', v, true))),
      field('Размер текста', select([['0.9', 'Компактный'], ['1', 'Обычный'], ['1.1', 'Крупный'], ['1.2', 'Очень крупный']], s.font_scale || '1', v => setSetting('font_scale', v, true))),
      field('Отдельный цвет кнопок', h('div', {class: 'row'}, h('input', {type: 'color', value: /^#[0-9a-f]{6}$/i.test(s.button_color || '') ? s.button_color : s.accent || '#6750A4', onchange: e => setSetting('button_color', e.target.value, true)}), h('button', {class: 'btn small', onclick: () => setSetting('button_color', '', true)}, 'Как акцент'))));
  }
  function iconPicker(initial, onPick) {
    const preview = h('span', {class: 'ico'}, recordIcon(initial));
    const changed = value => { onPick(value); preview.replaceChildren(recordIcon(value)); };
    const original = h('div', {class:'stack'});
    const grid = h('div', {class:'record-icon-grid',role:'group','aria-label':'Иконки записей'});
    for (const [key,[label]] of Object.entries(RECORDS)) grid.append(h('button',{type:'button',class:'chip','aria-label':label,'aria-pressed':String(initial === `builtin:${key}` || LEGACY[initial] === key),onclick: e => { grid.querySelectorAll('button').forEach(b=>b.setAttribute('aria-pressed',String(b===e.currentTarget))); changed(`builtin:${key}`); }},recordIcon(`builtin:${key}`)));
    original.append(grid);
    const input = h('input', {type: 'file', accept: 'image/png,image/jpeg,image/webp', 'aria-label': 'Загрузить свою иконку', onchange: e => safe(async () => {
      const file = e.target.files[0]; if (!file) return;
      if (file.size > 5 * 1024 * 1024) throw 'Иконка больше 5 МБ';
      const result = await api('icons.image', {data: await fileBase64(file)});
      changed(result.icon);
    })});
    original.prepend(h('div', {class: 'row'}, preview, field('Своя картинка', input)));
    return original;
  }
  async function undoButton() {
    const state = await api('history.status').catch(() => ({available: false}));
    return h('button', {type: 'button', class: 'icon-btn', disabled: !state.available, title: 'Отменить последнее изменение', 'aria-label': 'Отменить последнее изменение', onclick: () => safe(async () => { await api('history.undo'); syncSoon(); toast('Действие отменено'); render(); })}, icon('undo'));
  }
  function listCard() {
    const s = S.settings;
    const toggle = (key, label) => h('button', {class: 'chip', 'aria-pressed': String(s[key] === 'true'), onclick: () => setSetting(key, s[key] === 'true' ? 'false' : 'true', true)}, label);
    return h('div', {class: 'card settings-extra stack'}, h('div', {class: 'chips'}, toggle('list.group_dates', 'Разделять дела по датам'), toggle('list.hide_completed', 'Скрывать выполненные'), toggle('list.hide_archived', 'Скрывать архив')),
      field('Категория по умолчанию в «Делах»', select([['', 'Все категории'], ...S.cats.map(c => [String(c.id), c.name])], s['list.default_category'] || '', v => { S.filterCat = v ? Number(v) : null; setSetting('list.default_category', v, true); })),
      field('Сортировка дел', select([['date', 'По сроку'], ['title', 'По названию'], ['category', 'По категории']], s['list.sort'] || 'date', v => setSetting('list.sort', v, true))),
      field('Автоблокировка без действий', select([['0', 'Выключена'], ['1', 'Через минуту'], ['5', 'Через 5 минут'], ['15', 'Через 15 минут']], s['list.lock_minutes'] || '5', v => { setSetting('list.lock_minutes', v); resetIdle(); })),
      h('div', {class: 'row'}, h('button', {class: 'btn', onclick: passwordSheet}, 'Пароль на вход'), h('button', {class: 'btn', disabled: !lockEnabled, onclick: () => safe(lockNow)}, 'Заблокировать')));
  }
  async function filterReminders(all) {
    const query = (S.reminderQuery || '').trim().toLocaleLowerCase();
    let rows = all.filter(r => !query || `${r.title}\n${r.notes}`.toLocaleLowerCase().includes(query));
    if (S.settings['list.hide_completed'] === 'true') {
      const day = await api('day', {date: todayStr()});
      const completed = new Set(day.items.filter(i => i.status.startsWith('done') || i.status === 'skipped').map(i => i.reminder_id));
      rows = rows.filter(r => !completed.has(r.id));
    }
    if (S.settings['list.hide_archived'] === 'true') rows = rows.filter(r => !r.archived);
    return rows.sort((a,b) => S.settings['list.sort'] === 'title' ? a.title.localeCompare(b.title, 'ru') : S.settings['list.sort'] === 'category' ? ((catById(a.category_id)?.name || '').localeCompare(catById(b.category_id)?.name || '', 'ru') || a.start.localeCompare(b.start)) : a.start.localeCompare(b.start));
  }
  function remindersSearch() {
    return h('form', {class: 'row', onsubmit: e => { e.preventDefault(); render(); }}, h('input', {type: 'search', class: 'input grow', 'aria-label': 'Поиск дел', placeholder: 'Поиск по названию и описанию', value: S.reminderQuery || '', oninput: e => { S.reminderQuery = e.target.value; }}), h('button', {class: 'btn small', type: 'submit'}, 'Найти'));
  }
  function groupRows(rows, row) {
    if (S.settings['list.group_dates'] !== 'true') return h('div', {class: 'card list'}, ...rows.map(row));
    const groups = new Map();
    rows.forEach(r => { const key = r.repeat.type === 'none' ? longDate(fromYmd(r.start.slice(0,10))) : 'Повторяющиеся'; if (!groups.has(key)) groups.set(key, []); groups.get(key).push(r); });
    return h('div', {class: 'stack'}, ...[...groups].map(([title, rows]) => h('div', {class: 'stack'}, h('div', {class: 'section-title'}, `${title} · ${rows.length}`), h('div', {class: 'card list'}, ...rows.map(row)))));
  }
  function postpone(item) {
    const date = h('input', {type: 'datetime-local', value: `${todayStr()}T${pad(new Date().getHours())}:${pad(new Date().getMinutes())}`});
    const close = openSheet('Перенести это срабатывание', h('div', {class: 'stack'}, h('p', {class: 'muted small'}, 'Текущее срабатывание будет пропущено, а на выбранное время появится отдельное напоминание. Расписание повторов не меняется.'), field('Новый срок', date), h('button', {class: 'btn primary', onclick: () => safe(async () => { await api('postpone', {reminder_id: item.reminder_id, at: item.at, until: date.value}); close(); syncSoon(); toast('Перенесено'); render(); })}, 'Перенести')));
  }
  async function backupExport() {
    const result = await api('backup.export');
    if (!result.encrypted) { confirmSheet('Копия не зашифрована, так как группа ещё не создана. Сохранить открытый файл?', 'Сохранить', () => safe(() => saveFile(`dayplanner-${todayStr()}.dpbackup`, new Blob([result.text])))); return; }
    await saveFile(`dayplanner-${todayStr()}.dpbackup`, new Blob([result.text]));
  }
  function backupImport(file) {
    return safe(async () => {
      if (file.size > 40 * 1024 * 1024) throw 'Копия больше 40 МБ';
      const text = await file.text();
      confirmSheet('Восстановить записи и оформление из копии? Совпадающие записи заменятся, остальные сохранятся. Пароли и токены не затрагиваются.', 'Восстановить', () => safe(async () => { const r = await api('backup.restore', {text}); await init(); S.settings = await api('settings.get'); applyTheme(); syncSoon(); toast(`Восстановлено: ${r.applied}`); render(); }));
    });
  }
  async function dropboxCard() {
    return Collaboration.sharedCard();
  }
  function backupCard() {
    return h('div',{class:'card settings-extra stack'},h('p',{class:'muted small'},'Копия личных записей на этом устройстве. Общие категории хранятся отдельно и в эту копию не входят.'),h('button',{class:'btn',onclick:()=>safe(backupExport)},'Сохранить личную копию'),h('input',{type:'file',accept:'.dpbackup,application/json','aria-label':'Восстановить личную копию',onchange:e=>{if(e.target.files[0])backupImport(e.target.files[0]);}}));
  }
  return {init, apply, icon, recordIcon, iconPicker, undoButton, unlockGate, attachmentsEditor, attachmentList, customizationCard, listCard, filterReminders, remindersSearch, groupRows, postpone, dropboxCard, saveFile, fileBase64, backupCard};
})();
