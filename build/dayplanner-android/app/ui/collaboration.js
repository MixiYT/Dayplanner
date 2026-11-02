'use strict';
/* Shared categories never reuse the personal note/reminder API. */
const Collaboration = (() => {
  const safe = async fn => {try {return await fn();} catch(e) {toast(errText(e),'bad');}};
  const button = (label,fn,primary=false) => h('button',{type:'button',class:`btn${primary?' primary':''}`,onclick:e=>safe(async()=>{const b=e.currentTarget;b.disabled=true;try {await fn();}finally{b.disabled=false;}})},label);
  const details = (title,...nodes) => h('details',{class:'settings-section'},h('summary',{},title),h('div',{class:'stack details-body'},nodes));
  async function sharedCard() {
    const status=await api('dropbox.status'), categories=await api('shared.list');
    const list=h('div',{class:'stack'},...categories.map(c=>h('div',{class:'card settings-extra stack'},button(`${c.name}${c.remote?'':' · локальная копия'}${c.role==='viewer'?' · только чтение':''}`,()=>openCategory(c.id)),c.error?h('p',{class:'bad-text'},c.error):c.last_ok?h('p',{class:'muted small'},`Обновлено: ${fmtMs(c.last_ok)}`):null)));
    const root=h('div',{class:'stack'},h('p',{class:'muted'},'Совместные дела и заметки с другими людьми. Личная база и очки не публикуются.'),list);
    if(categories.length===0)root.append(h('div',{class:'card settings-extra muted'},'Пока нет общих категорий. Создайте новую или подключите приглашение.'));
    const key=h('input',{class:'input',placeholder:'App key Dropbox',autocomplete:'off','aria-label':'App key Dropbox'});
    const code=h('input',{class:'input',placeholder:'Код входа из Dropbox',autocomplete:'off','aria-label':'Код входа Dropbox'});
    const link=h('textarea',{class:'input',readonly:true,rows:'3','aria-label':'Ссылка для входа Dropbox'});
    const auth=details('Подключение Dropbox',h('p',{class:'muted small'},'Каждый участник входит в собственный аккаунт. Не передавайте токены другим людям. Для приложения Dropbox нужны Full Dropbox и разрешения sharing.read/write, files.metadata.read, files.content.read/write. App folder-токен из старой настройки не подходит.'),field('Ключ приложения Dropbox',key),button('Войти в браузере',async()=>{const r=await api('dropbox.auth_start',{app_key:key.value.trim()});link.value=r.url;if(!IS_DEMO)await TAURI.core.invoke('plugin:planner-native|native',{cmd:'open_url',args:{url:r.url}});}),link,field('Код, выданный Dropbox',code),button('Завершить вход',async()=>{await api('dropbox.auth_finish',{code:code.value.trim()});code.value='';toast('Dropbox подключён');render();},true),details('Дополнительно: импорт локальных настроек',h('input',{type:'file',accept:'.py,.json','aria-label':'Импорт настроек Dropbox',onchange:e=>safe(async()=>{const f=e.target.files[0];if(!f)return;if(f.size>32768)throw 'Файл больше 32 КБ';await api('dropbox.configure',{text:await f.text()});e.target.value='';toast('Настройки импортированы');render();})})));
    root.append(h('div',{class:'card settings-extra stack'},h('b',{},status.connected?'Dropbox подключён':'Dropbox не подключён'),h('p',{class:'muted small'},'Dropbox хранит только содержимое общих категорий. Оно доступно участникам папки и Dropbox; отдельного сквозного шифрования здесь нет.'),status.connected?button('Проверить доступ к общим папкам',async()=>{await api('dropbox.check');toast('Доступ разрешён');}):null,auth));
    if(status.connected)root.append(h('div',{class:'actions'},button('Создать общую категорию',createCategory,true),button('Найти приглашения',invitations)),details('Отключить аккаунт',button('Отключить Dropbox на этом устройстве',async()=>{await api('dropbox.disconnect');render();})));
    return root;
  }
  function createCategory() {
    const name=h('input',{class:'input',maxlength:'120',placeholder:'Например: Семья','aria-label':'Название общей категории'});
    const close=openSheet('Новая общая категория',h('div',{class:'stack'},field('Название',name),h('p',{class:'muted small'},'Создаётся пустая общая категория. Личные записи не копируются автоматически.'),button('Создать',async()=>{const r=await api('dropbox.create',{name:name.value});close();await openCategory(r.id);render();},true)));
  }
  async function invitations() {
    const folders=await api('dropbox.folders');
    openSheet('Общие папки Dropbox',h('div',{class:'stack'},h('p',{class:'muted small'},'Откройте приглашение в своём Dropbox, затем подключите папку здесь. Обычные папки без файла категории DayPlanner не подключаются.'),folders.length?folders.map(f=>button(f.name||f.id,async()=>{const r=await api('dropbox.attach',{remote:f.id});await openCategory(r.id);render();})):h('p',{},'Приглашений пока нет.')));
  }
  async function openCategory(id) {
    const cat=await api('shared.get',{id});
    const edit=['owner','editor','local'].includes(cat.role);
    const root=h('div',{class:'stack'});
    let close;
    const reload=async()=>{close();await openCategory(id);};
    root.append(h('p',{class:'muted small'},cat.remote?'Общая категория · изменения обновляются раз в минуту, пока приложение открыто. Чужие выполнения не дают личных очков.':'Локальная копия · изменения больше не отправляются в Dropbox.'));
    if(cat.remote)root.append(button('Обновить общую категорию',async()=>{await api('dropbox.category_run',{id});await reload();}));
    if(!edit)root.append(h('b',{},'Только чтение'));
    const rows=Object.values(cat.document.entries).filter(e=>!e.deleted).sort((a,b)=>a.title.localeCompare(b.title,'ru'));
    for(const kind of ['task','note'])root.append(h('h3',{},kind==='task'?'Дела':'Заметки'),...rows.filter(e=>e.kind===kind).map(e=>h('div',{class:'card shared-row'},e.kind==='task'?h('input',{type:'checkbox',checked:e.done,disabled:!edit,'aria-label':`Выполнено: ${e.title}`,onchange:ev=>safe(async()=>{try{await api('shared.save',{id,expected:e.modified,entry:{...e,done:ev.target.checked}});await reload();}catch(err){ev.target.checked=e.done;throw err;}})}):null,h('span',{},FeatureUI.recordIcon(e.icon)),h('div',{class:'grow stack'},h('b',{},e.title),e.body?h('p',{class:'shared-text'},e.body):null,FeatureUI.attachmentList(e.attachments)),button(edit?'Изменить':'Открыть',()=>entryEditor(id,e,edit,reload)))));
    if(edit)root.append(h('div',{class:'actions'},button('Добавить дело',()=>entryEditor(id,null,true,reload,'task'),true),button('Добавить заметку',()=>entryEditor(id,null,true,reload,'note'))));
    if(cat.role==='owner'&&cat.remote)root.append(button('Пригласить участника',()=>invite(id)));
    if(cat.remote)root.append(details('Отключение',button('Отключить и оставить локальную копию',()=>confirmSheet('Категория останется здесь как отдельная локальная копия. Общие данные других людей не удаляются. Для отзыва доступа к папке используйте Dropbox.', 'Отключить',async()=>{await api('shared.detach',{id});await reload();render();}))));
    root.append(h('p',{class:'muted small'},'Одновременные изменения разных записей объединяются. Если одну запись изменяют одновременно, побеждает последняя правка целиком.'));
    close=openSheet(cat.document.name,root);
  }
  function entryEditor(id,existing,editable,after,kind='task') {
    const e=existing?JSON.parse(JSON.stringify(existing)):{id:'',kind,title:'',body:'',icon:`builtin:${kind}`,done:false,deleted:false,modified:0,author:'',attachments:[]};
    const title=h('input',{class:'input',value:e.title,maxlength:'500',disabled:!editable,'aria-label':'Название общей записи'});
    const body=h('textarea',{class:'input',rows:'6',disabled:!editable,'aria-label':'Текст общей записи'},e.body);
    const files=editable?FeatureUI.attachmentsEditor(e):FeatureUI.attachmentList(e.attachments);
    let close;
    const save=async deleted=>{await files.ready?.();e.title=title.value.trim();e.body=body.value;e.deleted=deleted;if(!deleted&&!e.title&&!e.body)throw 'Введите название или текст';await api('shared.save',{id,expected:existing?.modified||0,entry:e});close();await after();};
    close=openSheet(existing?'Общая запись':'Новая общая запись',h('div',{class:'stack'},field('Название',title),body,editable?FeatureUI.iconPicker(e.icon,v=>e.icon=v):null,files,editable?h('div',{class:'actions'},button('Сохранить локальную правку',()=>save(false),true),existing?button('Удалить',()=>confirmSheet('Удалить запись у всех участников категории?','Удалить',()=>save(true))):null):null));
  }
  function invite(id) {
    const email=h('input',{type:'email',class:'input',autocomplete:'email','aria-label':'Email участника'}),readonly=h('input',{type:'checkbox'});
    const close=openSheet('Пригласить в категорию',h('div',{class:'stack'},field('Email аккаунта Dropbox',email),h('label',{class:'row'},readonly,'Только чтение'),button('Отправить приглашение',async()=>{await api('dropbox.invite',{id,email:email.value,readonly:readonly.checked});close();toast('Приглашение отправлено через Dropbox');},true)));
  }
  async function launch() {openSheet('Общие категории',await sharedCard());}
  async function lanPermission() {
    if(IS_ANDROID&&!IS_DEMO) {const r=await TAURI.core.invoke('plugin:planner-native|native',{cmd:'lan_status',args:{}});if(!r?.granted){await native('request',{kind:'localNetwork'});throw 'Разрешите доступ к локальной сети, затем повторите поиск.';}}
  }
  async function pairing(host) {
    await lanPermission();
    if(IS_DEMO)throw 'Подключение устройств работает в установленном приложении';
    if(host)await api('pairing.host');
    const panel=h('div',{class:'stack'});let stopped=false,timer=0;
    const close=openSheet('Подключить своё устройство',h('div',{class:'stack'},h('p',{},'Оба устройства должны быть в одной Wi-Fi сети. На втором нажмите «Найти устройство». Сравните 8 цифр на обоих экранах — подтверждайте только при полном совпадении.'),h('p',{class:'muted small'},'Это объединит личные данные устройств. Для совместной работы с другим человеком используйте «Общие категории».'),panel),()=>{stopped=true;clearTimeout(timer);api('pairing.cancel').catch(()=>{});});
    const poll=async()=>{
      if(stopped)return;
      try {const s=await api('pairing.status');if(stopped)return;
        if(s.mode==='done'){panel.replaceChildren(h('b',{},'Устройства связаны'),button('Готово',async()=>{close();syncSoon();render();}));return;}
        if(s.mode==='error'||(s.mode&&s.remaining===0)){panel.replaceChildren(h('p',{class:'bad-text'},s.error||'Время подключения истекло. Начните заново.'));return;}
        if(s.sas)panel.replaceChildren(h('b',{},s.peer),h('div',{class:'pairing-sas','aria-label':'Проверочный код'},s.sas.slice(0,4)+' '+s.sas.slice(4)),s.approved?h('p',{},'Ждём подтверждение на втором устройстве…'):button('Цифры совпадают на обоих экранах',()=>api('pairing.confirm',{sas:s.sas}),true),button('Не совпадают — отменить',async()=>close()));
        else if(host)panel.replaceChildren(h('p',{},`Ожидаем второе устройство · ${s.remaining} с`),s.error?h('p',{class:'bad-text'},s.error):null);
      }catch(e){if(!stopped)panel.replaceChildren(h('p',{class:'bad-text'},errText(e)));}
      if(!stopped)timer=setTimeout(poll,800);
    };
    if(!host) {
      const devices=await api('pairing.find');if(stopped)return;
      panel.replaceChildren(devices.length?h('div',{class:'stack'},...devices.map(d=>button(d.name,async()=>{await api('pairing.connect',{addr:d.addr,session:d.session});await poll();}))):h('p',{},'Устройства не найдены. На другом устройстве нажмите «Показать это устройство». Проверьте Wi-Fi, разрешение локальной сети и брандмауэр.'));
    } else await poll();
  }
  function devicesCard() {return h('div',{class:'card settings-extra stack'},h('b',{},'Мои устройства'),h('p',{class:'muted small'},'Личные дела, заметки и расписание на телефоне и ПК. Без ручного ввода IP, с подтверждением на обоих экранах.'),h('div',{class:'actions'},button('Показать это устройство',()=>pairing(true),true),button('Найти устройство',()=>pairing(false))),h('p',{class:'muted small'},'Поиск включается только по вашему действию на 3 минуты. После подключения данные передаются автоматически по локальной сети.'));
  }
  return {sharedCard,launch,devicesCard,details};
})();
