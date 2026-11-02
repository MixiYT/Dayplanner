/* Демо-бэкенд для запуска интерфейса в обычном браузере (без Tauri/Rust).
   Нужен только для разработки и проверки вёрстки: повторяет формат ответов ядра,
   но логика упрощена. В настоящем приложении не используется. */
'use strict';

const demo = (() => {
  const pad = (n) => String(n).padStart(2, '0');
  const ymd = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  const parseDT = (s) => { const [d, t] = s.split('T'); const [y, m, dd] = d.split('-').map(Number); const [hh, mm] = t.split(':').map(Number); return new Date(y, m - 1, dd, hh, mm, 0); };
  const fmtDT = (d) => `${ymd(d)}T${pad(d.getHours())}:${pad(d.getMinutes())}:00`;
  const addDays = (d, n) => { const x = new Date(d); x.setDate(x.getDate() + n); return x; };
  const KEY = 'dayplanner-demo-v1';
  let db = null;

  function seed() {
    const today = new Date();
    const at = (h, m = 0, off = 0) => { const d = addDays(today, off); d.setHours(h, m, 0, 0); return fmtDT(d); };
    return {
      seq: 100,
      categories: [
        { id: 1, name: 'Работа', color: '#4285F4', icon: '💼' },
        { id: 2, name: 'Учёба', color: '#9C27B0', icon: '🎓' },
        { id: 3, name: 'Здоровье', color: '#E53935', icon: '❤️' },
        { id: 4, name: 'Дом', color: '#43A047', icon: '🏠' },
        { id: 5, name: 'Личное', color: '#FB8C00', icon: '⭐' },
      ],
      notes: [
        { id: 50, title: 'Покупки', body: 'Молоко\nХлеб\nЯблоки\nКофе в зёрнах', color: '#43A047', pinned: true, category_id: 4, created: at(9, 0, -2), updated: at(9, 0, -1) },
        { id: 51, title: 'Идея для приложения', body: 'Сделать общий рейтинг для друзей и недельные челленджи. Добавить совместные списки дел.', color: '#1E88E5', pinned: false, category_id: 1, created: at(12, 0, -3), updated: at(12, 30, -3) },
        { id: 52, title: '', body: 'Позвонить в поликлинику и записаться на приём к врачу на следующей неделе', color: null, pinned: false, category_id: 3, created: at(8, 0, 0), updated: at(8, 15, 0) },
        { id: 53, title: 'Книги', body: '1. Дюна\n2. Задача трёх тел\n3. Мастер и Маргарита', color: '#F9A825', pinned: false, category_id: 5, created: at(20, 0, -6), updated: at(20, 5, -6) },
      ],
      reminders: [
        { id: 1, title: 'Выпить таблетки', notes: '', icon: '💊', category_id: 3, start: at(8, 30, -5), alert: 'alarm', repeat: { type: 'daily' }, repeat_until: null, grace_minutes: 60, archived: false },
        { id: 2, title: 'Созвон с командой', notes: 'Обсудить релиз', icon: '💼', category_id: 1, start: at(11, 0, -9), alert: 'full_screen', repeat: { type: 'weekdays', days: [1, 3, 5] }, repeat_until: null, grace_minutes: 30, archived: false },
        { id: 3, title: 'Тренировка', notes: '', icon: '🏋️', category_id: 3, start: at(18, 30, -9), alert: 'notification', repeat: { type: 'weekdays', days: [2, 4, 6] }, repeat_until: null, grace_minutes: 120, archived: false },
        { id: 4, title: 'Оплатить интернет', notes: '', icon: '💳', category_id: 4, start: at(10, 0, -20), alert: 'notification', repeat: { type: 'monthly' }, repeat_until: null, grace_minutes: 1440, archived: false },
        { id: 5, title: 'Прочитать 20 страниц', notes: '', icon: '📖', category_id: 5, start: at(22, 15, -5), alert: 'notification', repeat: { type: 'daily' }, repeat_until: null, grace_minutes: 60, archived: false },
      ],
      schedule: [
        { id: 1, title: 'Матанализ', weekday: 1, date: null, start: '09:00', end: '10:30', color: null, icon: '📐', location: 'ауд. 214', category_id: 2 },
        { id: 2, title: 'Программирование', weekday: 1, date: null, start: '10:45', end: '12:15', color: null, icon: '💻', location: 'ауд. 305', category_id: 2 },
        { id: 3, title: 'Английский', weekday: 2, date: null, start: '09:00', end: '10:30', color: null, icon: '🇬🇧', location: 'ауд. 118', category_id: 2 },
        { id: 4, title: 'Физика', weekday: 2, date: null, start: '10:00', end: '11:30', color: null, icon: '⚛️', location: 'ауд. 220', category_id: 2 },
        { id: 5, title: 'Семинар', weekday: 3, date: null, start: '13:00', end: '14:30', color: null, icon: '🗣️', location: 'ауд. 101', category_id: 2 },
        { id: 6, title: 'Лабораторная', weekday: 4, date: null, start: '12:00', end: '15:00', color: null, icon: '🔬', location: 'лаб. 3', category_id: 2 },
        { id: 7, title: 'Проект', weekday: 5, date: null, start: '09:00', end: '12:00', color: null, icon: '🛠️', location: '', category_id: 1 },
      ],
      completions: [], unlocked: {},
      settings: {},
    };
  }
  function load() {
    if (db) return db;
    try { db = JSON.parse(localStorage.getItem(KEY)); } catch (_) { db = null; }
    if (!db) db = seed();
    return db;
  }
  function save() { try { localStorage.setItem(KEY, JSON.stringify(db)); } catch (_) { /* нет хранилища */ } }

  function occ(r, from, to) {
    const out = [];
    const start = parseDT(r.start);
    const sd = new Date(start.getFullYear(), start.getMonth(), start.getDate());
    for (let d = new Date(Math.max(from, sd)); d <= to; d = addDays(d, 1)) {
      if (r.repeat_until && ymd(d) > r.repeat_until) break;
      let ok = false;
      const wd = ((d.getDay() + 6) % 7) + 1;
      const last = new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate();
      switch (r.repeat.type) {
        case 'daily': ok = true; break;
        case 'weekdays': ok = r.repeat.days.includes(wd); break;
        case 'monthly': ok = d.getDate() === Math.min(sd.getDate(), last); break;
        default: ok = ymd(d) === ymd(sd);
      }
      if (ok) out.push(new Date(d.getFullYear(), d.getMonth(), d.getDate(), start.getHours(), start.getMinutes()));
    }
    return out;
  }
  const comp = (id, at) => load().completions.find((c) => c.reminder_id === id && c.at === fmtDT(at));
  function status(r, at, now) {
    const c = comp(r.id, at);
    const dl = new Date(at.getTime() + r.grace_minutes * 60000);
    if (c) { if (c.kind === 'skipped') return 'skipped'; return parseDT(c.completed_at) <= dl ? 'done_on_time' : 'done_late'; }
    if (now < at) return 'upcoming';
    return now <= dl ? 'due' : 'missed';
  }
  function dayView(date, now) {
    const d = new Date(date + 'T00:00:00');
    const wd = ((d.getDay() + 6) % 7) + 1;
    const entries = load().schedule.filter((e) => e.date === date || (!e.date && e.weekday === wd))
      .sort((a, b) => a.start.localeCompare(b.start));
    const items = [];
    load().reminders.filter((r) => !r.archived).forEach((r) => occ(r, d, d).forEach((at) => items.push({
      reminder_id: r.id, title: r.title, notes: r.notes, icon: r.icon, category_id: r.category_id, at: fmtDT(at),
      alert: r.alert, status: status(r, at, now),
    })));
    items.sort((a, b) => a.at.localeCompare(b.at));
    return { date, entries, items };
  }
  function stats(now) {
    const cs = load().completions.filter((c) => c.kind === 'done');
    const points = cs.reduce((s, c) => s + c.points, 0) + Object.keys(load().unlocked).length * 20;
    const level = Math.floor(Math.sqrt(points / 50)) + 1;
    const lo = (level - 1) ** 2 * 50, hi = level ** 2 * 50;
    const today = dayView(ymd(now), now);
    return {
      total_done: cs.length, on_time: cs.length, late: 0, missed: 0, skipped: 0, current_streak: Math.min(cs.length, 4),
      best_streak: Math.min(cs.length, 4), perfect_days: 0, early_bird: 0, night_owl: 0, reminders_created: load().reminders.length,
      categories_used: 4, points, level, level_progress: (points - lo) / (hi - lo),
      today_total: today.items.length, today_done: today.items.filter((i) => i.status.startsWith('done')).length,
    };
  }
  const DEFS = [
    ['first_step', '🌱', 'Первый шаг', 'Выполните первое напоминание', 1],
    ['done_10', '💪', 'Втянулся', 'Выполните 10 напоминаний', 10],
    ['streak_7', '🔥', 'Неделя огня', 'Серия из 7 дней без пропусков', 7],
    ['done_100', '💯', 'Сотня', 'Выполните 100 напоминаний', 100],
  ];

  function api(cmd, a, nowOverride) {
    const d = load();
    const now = nowOverride ? parseDT(nowOverride) : new Date();
    const nid = () => ++d.seq;
    switch (cmd) {
      case 'security.status': return {enabled: false, locked: false};
      case 'security.unlock': return {enabled: false, locked: false};
      case 'security.password': throw 'Пароль доступен только в установленном приложении';
      case 'security.lock': return {enabled: false, locked: false};
      case 'history.status': return {available: !!d.history};
      case 'history.undo': { if (!d.history) throw 'Нет действия для отмены'; db = d.history; save(); return {applied: 1}; }
      case 'icons.get': return {};
      case 'icons.reset': return null;
      case 'icons.import': throw 'Обработка ZIP доступна в приложении; в демо можно посмотреть встроенные наборы';
      case 'dropbox.status': return {connected: false, auto: false};
      case 'shared.list': return [];
      case 'pairing.status': return {mode:'',remaining:0};
      case 'backup.export': return {text: JSON.stringify({format:'dayplanner-demo-backup',data:{...d,history:undefined}}),encrypted:false};
      case 'backup.restore': { const v=JSON.parse(a.text); if(v.format!=='dayplanner-demo-backup') throw 'Нужна копия демо-режима'; db=v.data; save(); return {applied:1}; }
      case 'now': return fmtDT(now);
      case 'categories.list': return d.categories;
      case 'categories.save': { const c = a.category; if (!c.name.trim()) throw 'Название категории не может быть пустым'; if (!c.id) { c.id = nid(); d.categories.push(c); } else d.categories = d.categories.map((x) => (x.id === c.id ? c : x)); save(); return c; }
      case 'categories.delete': d.categories = d.categories.filter((c) => c.id !== a.id); save(); return null;
      case 'reminders.list': return d.reminders.filter((r) => a.archived || !r.archived);
      case 'reminders.save': {
        const r = a.reminder; if (!r.title.trim()) throw 'Название не может быть пустым';
        r.start = fmtDT(parseDT(r.start));
        if (!r.id) { r.id = nid(); d.reminders.push(r); } else d.reminders = d.reminders.map((x) => (x.id === r.id ? r : x));
        save(); return r;
      }
      case 'reminders.delete': d.reminders = d.reminders.filter((r) => r.id !== a.id); save(); return null;
      case 'reminders.archive': d.reminders.forEach((r) => { if (r.id === a.id) r.archived = a.archived; }); save(); return null;
      case 'postpone': { if(parseDT(a.until)<=now) throw 'Новый срок должен быть в будущем'; const old=d.reminders.find(r=>r.id===a.reminder_id); if(!old) throw 'Не найдено'; d.reminders.push({...old,id:nid(),start:fmtDT(parseDT(a.until)),repeat:{type:'none'},repeat_until:null,archived:false}); if(old.repeat.type==='none') old.archived=true; d.completions.push({reminder_id:old.id,at:fmtDT(parseDT(a.at)),kind:'skipped',completed_at:fmtDT(now),points:0}); save(); return null; }
      case 'sync.status': { const sy = d.sync || {}; return { enabled: !!sy.code, code: sy.code || null, ntfy: sy.ntfy !== false, lan: sy.lan !== false, server: sy.server || 'https://ntfy.sh', device: 'demo', last_ok: sy.last_ok || null, last_lan: null, error: null, lan_addr: '192.168.1.20:47831' }; }
      case 'sync.create': { d.sync = Object.assign(d.sync || {}, { code: 'DEMO-7K3P-XQ4M-2WNH' }); save(); return { code: d.sync.code }; }
      case 'sync.join': { if (!/^[A-Za-z0-9 -]{16,24}$/.test(a.code || '')) throw 'Код группы состоит из 16 букв и цифр, например ABCD-EFGH-JKLM-NPQR'; d.sync = Object.assign(d.sync || {}, { code: a.code.toUpperCase() }); save(); return { code: d.sync.code }; }
      case 'sync.leave': d.sync = {}; save(); return null;
      case 'sync.config': d.sync = Object.assign(d.sync || {}, { ntfy: a.ntfy, lan: a.lan, server: (a.server || 'https://ntfy.sh').replace(/\/$/, '') }); save(); return null;
      case 'sync.export': return { text: '{"v":1,"device":"demo","msg":"data","records":[],"unlocked":[]}' };
      case 'sync.import': throw 'Это не файл синхронизации ежедневника';
      case 'sync.run': d.sync = Object.assign(d.sync || {}, { last_ok: Date.now() }); save(); return { applied: 0, pushed: 0 };
      case 'sync.lan_connect': throw 'В демо-режиме недоступно';
      case 'notes.list': {
        const q = (a.query || '').trim().toLowerCase();
        return (d.notes || []).filter((n) => (a.category_id == null || n.category_id === a.category_id) && (!q || n.title.toLowerCase().includes(q) || n.body.toLowerCase().includes(q)))
          .sort((x, y) => (y.pinned - x.pinned) || (x.updated < y.updated ? 1 : x.updated > y.updated ? -1 : y.id - x.id));
      }
      case 'notes.save': {
        const n = a.note; if (!n.title.trim() && !n.body.trim()) throw 'Заметка пуста: добавьте заголовок или текст';
        d.notes = d.notes || [];
        n.title = n.title.trim(); n.color = n.color || null; n.updated = fmtDT(now);
        if (!n.id) { n.id = nid(); n.created = fmtDT(now); d.notes.push(n); } else { const old = d.notes.find((x) => x.id === n.id); n.created = old ? old.created : fmtDT(now); d.notes = d.notes.map((x) => (x.id === n.id ? n : x)); }
        save(); return n;
      }
      case 'notes.delete': d.notes = (d.notes || []).filter((n) => n.id !== a.id); save(); return null;
      case 'schedule.list': return d.schedule;
      case 'schedule.save': {
        const e = a.entry; if (!e.title.trim()) throw 'Название не может быть пустым';
        if (e.end <= e.start) throw 'Конец должен быть позже начала';
        if (!e.id) { e.id = nid(); d.schedule.push(e); } else d.schedule = d.schedule.map((x) => (x.id === e.id ? e : x));
        save(); return e;
      }
      case 'schedule.delete': d.schedule = d.schedule.filter((e) => e.id !== a.id); save(); return null;
      case 'day': return dayView(a.date, now);
      case 'week': {
        const dt = new Date(a.date + 'T00:00:00'); const mon = addDays(dt, -((dt.getDay() + 6) % 7));
        return { start: ymd(mon), days: [0, 1, 2, 3, 4, 5, 6].map((i) => dayView(ymd(addDays(mon, i)), now)) };
      }
      case 'pending': { const out = []; for (let i = 0; i <= 3; i++) out.push(...dayView(ymd(addDays(now, -i)), now).items.filter((x) => x.status === 'due' || x.status === 'missed')); return out; }
      case 'complete': {
        const r = d.reminders.find((x) => x.id === a.reminder_id); const at = parseDT(a.at);
        if (comp(r.id, at)) throw 'Уже выполнено';
        const onTime = now <= new Date(at.getTime() + r.grace_minutes * 60000);
        const c = { reminder_id: r.id, at: fmtDT(at), kind: 'done', completed_at: fmtDT(now), points: onTime ? 10 : 4 };
        d.completions.push(c);
        const unlocked = []; const total = d.completions.filter((x) => x.kind === 'done').length;
        DEFS.forEach(([id, icon, title, description, target]) => {
          if (!d.unlocked[id] && total >= target && id !== 'streak_7') { d.unlocked[id] = fmtDT(now); unlocked.push({ id, icon, title, description, reward: 20, target, progress: target, unlocked: true }); }
        });
        save();
        return { status: onTime ? 'done_on_time' : 'done_late', points: c.points, total_points: stats(now).points, unlocked };
      }
      case 'skip': d.completions.push({ reminder_id: a.reminder_id, at: fmtDT(parseDT(a.at)), kind: 'skipped', completed_at: fmtDT(now), points: 0 }); save(); return null;
      case 'undo': d.completions = d.completions.filter((c) => !(c.reminder_id === a.reminder_id && c.at === fmtDT(parseDT(a.at)))); save(); return null;
      case 'stats': return stats(now);
      case 'achievements': { const s = stats(now); return DEFS.map(([id, icon, title, description, target]) => ({ id, icon, title, description, reward: 20, target, progress: Math.min(target, s.total_done), unlocked: !!d.unlocked[id], unlocked_at: d.unlocked[id] || null })); }
      case 'settings.get': return Object.assign({ theme: 'material', accent: '#6750A4', mode: 'auto', bg: 'aurora', bg_intensity: '60', 'online.server': '', 'online.room': '', 'online.nickname': '', 'online.registered': 'false' }, d.settings);
      case 'settings.set': d.settings[a.key] = a.value; save(); return null;
      case 'alarms': return [];
      case 'widget': return {};
      default: if (cmd.startsWith('online.')) throw 'Рейтинг недоступен в демо-режиме'; if(cmd.startsWith('dropbox.')) throw 'Dropbox доступен только в приложении; токены в браузере не сохраняются'; throw `неизвестная команда: ${cmd}`;
    }
  }
  return { api: async (c, a = {}, n) => {
    if(c==='attachments.prepare') { const bytes=Uint8Array.from(atob(a.data),x=>x.charCodeAt(0)); if(bytes.length>10*1024*1024) throw 'Файл больше 10 МБ'; const digest=new Uint8Array(await crypto.subtle.digest('SHA-256',bytes)); return {...a,id:[...digest].map(x=>x.toString(16).padStart(2,'0')).join('')}; }
    if(c==='icons.image') { const img=new Image(); img.src=`data:image/png;base64,${a.data}`; await img.decode(); if(img.width>4096||img.height>4096) throw 'Изображение слишком большое'; const canvas=document.createElement('canvas'),scale=Math.min(1,192/Math.max(img.width,img.height)); canvas.width=Math.max(1,Math.round(img.width*scale));canvas.height=Math.max(1,Math.round(img.height*scale));canvas.getContext('2d').drawImage(img,0,0,canvas.width,canvas.height); return {icon:canvas.toDataURL('image/png')}; }
    const undoable=['categories.save','categories.delete','reminders.save','reminders.delete','reminders.archive','notes.save','notes.delete','schedule.save','schedule.delete','complete','skip','undo','postpone'].includes(c);
    const before=undoable?JSON.parse(JSON.stringify({...load(),history:undefined})):null;
    const result=api(c,a,n);
    if(undoable){db.history=before;save();}
    return result;
  }, reset() { db = seed(); save(); } };
})();
