/* Browser smoke tests. Only the demo store in this test origin is modified. */
async function runFeatureTests() {
  document.querySelector('#previews').replaceChildren();
  const frame = await makePreview('pc');
  const win = frame.contentWindow, doc = frame.contentDocument;
  frame.width = 393; frame.height = 852;
  win.eval('demo.reset()');
  const failures = [];
  let checks = 0;
  const check = (ok, label) => { checks++; if (!ok) failures.push(label); };
  await win.eval("S.tab='settings'; render()");
  check(doc.querySelector('#view').textContent.includes('Мои устройства'), 'Понятные разделы настроек');
  check(!doc.querySelector('#view').textContent.includes('Dropbox — дополнительный способ синхронизации'), 'Старое описание Dropbox отсутствует');
  await win.eval("S.settingsSection='shared';render()");
  check(doc.querySelector('#view').textContent.includes('Совместные дела и заметки'), 'Dropbox как общие категории');
  await win.eval("S.settingsSection='appearance';render()");
  check(doc.querySelector('#view').textContent.includes('Иконки, шрифты'), 'Настройки оформления');
  const resourcePaths = new Set();
  for (const pack of ['default', 'rounded', 'minimalistic', 'minimalistic-blue', 'old', 'custom']) {
    for (const theme of THEMES) for (const mode of ['light', 'dark']) {
      await win.api('settings.set', {key: 'icon_pack', value: pack});
      await win.api('settings.set', {key: 'theme', value: theme});
      await win.api('settings.set', {key: 'mode', value: mode});
      await win.eval("api('settings.get').then(s=>{S.settings=s;applyTheme();drawChrome()})");
      const nav = doc.querySelector('#nav').getBoundingClientRect();
      check(nav.height <= 80 && nav.left >= 0 && nav.right <= 393, `Панель: ${pack}/${theme}/${mode}`);
      const preview = win.eval("FeatureUI.customizationCard()");
      preview.querySelectorAll('img').forEach(img => resourcePaths.add(img.src));
      preview.remove();
    }
  }
  for (const path of resourcePaths) check((await fetch(path)).ok, `Ресурс иконки ${path}`);
  for (const family of ['system', 'sans', 'serif', 'ubuntu', 'mono']) {
    await win.api('settings.set', {key: 'font_family', value: family});
    await win.eval("api('settings.get').then(s=>{S.settings=s;applyTheme()})");
    const font = win.getComputedStyle(doc.body).fontFamily;
    check(!!font, `Шрифт ${family}`);
  }
  const body = 'Attachment contents';
  const file = new win.File([body], 'example.txt', {type: 'text/plain'});
  await win.eval("S.tab='notes'; render()");
  win.noteSheet(null);
  check(doc.querySelector('.sheet-body') && getComputedStyle(doc.querySelector('.sheet-head')).position !== 'sticky', 'Редактор прокручивается отдельно от шапки');
  check(doc.querySelector('.record-icon-grid button') && !doc.querySelector('.emoji-grid'), 'Встроенные иконки вместо эмодзи');
  doc.querySelector('[aria-label="Заголовок"]').value = 'Feature test attachment';
  doc.querySelector('[aria-label="Текст заметки"]').value = 'Test note';
  const input = doc.querySelector('.attachments-editor input[type=file]');
  const dt = new win.DataTransfer(); dt.items.add(file); input.files = dt.files;
  input.dispatchEvent(new win.Event('change', {bubbles: true}));
  await doc.querySelector('.attachments-editor').ready();
  check(doc.querySelector('.attachment-list').textContent.includes('example.txt'), 'Добавление файла в редактор');
  [...doc.querySelectorAll('.sheet .btn')].find(b => b.textContent === 'Сохранить').click();
  await nextFrame(win);
  let notes = await win.api('notes.list', {});
  const note = notes.find(n => n.title === 'Feature test attachment');
  check(!!note && atob(note.attachments[0].data) === body, 'Сохранение исходных байт вложения');
  check((await win.api('history.status')).available, 'Отмена доступна');
  await win.api('history.undo');
  check(!(await win.api('notes.list', {})).some(n => n.title === 'Feature test attachment'), 'Отмена создания заметки');
  const image = doc.createElement('canvas'); image.width = 400; image.height = 300;
  const uploaded = await win.api('icons.image', {data: image.toDataURL('image/png').split(',')[1]});
  const node = win.eval('FeatureUI.recordIcon')(uploaded.icon);
  check(node.tagName === 'IMG', 'Загруженная иконка отображается картинкой');
  const r = (await win.api('reminders.list', {archived: true}))[0];
  await win.api('postpone', {reminder_id: r.id, at: r.start, until: '2099-12-31T12:00'});
  check((await win.api('reminders.list', {archived: true})).some(r => r.start.startsWith('2099-12-31')), 'Перенос срабатывания');
  await win.api('history.undo');
  await win.api('settings.set', {key: 'button_color', value: '#ffffff'});
  await win.eval("api('settings.get').then(s=>{S.settings=s;applyTheme()})");
  check(doc.documentElement.style.getPropertyValue('--on-button') === '#161616', 'Контраст текста на светлой кнопке');
  await win.eval("S.tab='settings'; render()");
  const result = {checks, failures};
  document.querySelector('#result').textContent = JSON.stringify(result, null, 2);
  return result;
}
