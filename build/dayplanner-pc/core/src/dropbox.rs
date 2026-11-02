//! Dropbox is exclusively for shared categories, not personal device sync.
use std::{io::Read, sync::{Arc,Mutex}, time::Duration};
use chrono::Utc;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use crate::{Store,Result,error::Error,features::{invalid,random},sync,shared::Document};
const MAX:usize=40*1024*1024;
#[cfg(test)]
thread_local! { static TEST_ENDPOINT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) }; }
fn endpoint(content:bool,method:&str)->String {
    #[cfg(test)] if let Some(base)=TEST_ENDPOINT.with(|v|v.borrow().clone()) {return format!("{base}/{method}");}
    format!("https://{}.dropboxapi.com/2/{method}",if content {"content"} else {"api"})
}
fn lock(s:&Mutex<Store>)->Result<std::sync::MutexGuard<'_,Store>> {s.lock().map_err(|_|invalid("Хранилище недоступно"))}
fn agent()->ureq::Agent {ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).redirects(0).build()}
fn error(e:ureq::Error)->Error {match e {
    ureq::Error::Status(401,_)=>invalid("Dropbox: войдите в аккаунт заново"),
    ureq::Error::Status(403,_)=>invalid("Dropbox: для общих категорий нужен Full Dropbox и разрешения sharing.read/write, files.metadata.read, files.content.read/write. App folder не подходит. Измените Dropbox App и войдите заново."),
    ureq::Error::Status(409,_)=>invalid("Dropbox: операция недоступна. Проверьте приглашение, права и тип Dropbox App (Full Dropbox)."),
    ureq::Error::Status(429,_)=>invalid("Dropbox: слишком много запросов. Повторите позже"),
    _=>Error::Net("Нет связи с Dropbox".into())
}}
fn bytes(r:ureq::Response,max:usize)->Result<Vec<u8>> {let mut b=vec![];r.into_reader().take(max as u64+1).read_to_end(&mut b).map_err(|_|invalid("Не удалось прочитать Dropbox"))?;if b.len()>max {return Err(invalid("Файл Dropbox слишком большой"));}Ok(b)}
fn body(r:ureq::Response)->Result<Value> {Ok(serde_json::from_slice(&bytes(r,1024*1024)?)?)}
fn request(t:&str,m:&str,v:&Value)->Result<Value> {body(agent().post(&endpoint(false,m)).set("Authorization",&format!("Bearer {t}")).send_json(v).map_err(error)?)}
pub fn parse_credentials(text:&str)->Result<Value> {
    if text.len()>32768 {return Err(invalid("Файл настроек больше 32 КБ"));}
    let names=[("DROPBOX_APP_KEY","app_key"),("DROPBOX_REFRESH_TOKEN","refresh_token"),("DROPBOX_ACCESS_TOKEN","access_token"),("DROPBOX_APP_SECRET","app_secret")];
    let mut out=json!({});
    if text.trim_start().starts_with('{') {let v:Value=serde_json::from_str(text)?;for (n,k) in names {out[k]=v.get(k).or_else(||v.get(n)).cloned().unwrap_or(json!(""));}}
    else {for line in text.lines() {let Some((n,v))=line.trim().split_once('=') else {continue};let Some((_,k))=names.iter().find(|(n2,_)|*n2==n.trim()) else {continue};let v=v.trim();let q=v.chars().next().unwrap_or(' ');if q!='\''&&q!='"' {return Err(invalid("Допустимы только строковые литералы, Python не исполняется"));}let end=v[1..].find(q).ok_or_else(||invalid("Незакрытая строка"))?+1;let tail=v[end+1..].trim();if !tail.is_empty()&&!tail.starts_with('#') {return Err(invalid("Python-выражения запрещены"));}out[*k]=json!(&v[1..end]);}}
    validate_credentials(&out)?;Ok(out)
}
fn validate_credentials(v:&Value)->Result<()> {
    for k in ["app_key","refresh_token","access_token","app_secret"] {if let Some(x)=v.get(k) {let s=x.as_str().ok_or_else(||invalid("Настройки должны быть строками"))?;if s.len()>8192||!s.chars().all(|c|c.is_ascii_graphic()) {return Err(invalid("Недопустимые настройки Dropbox"));}}}
    let has=|k:&str|v[k].as_str().is_some_and(|s|!s.is_empty());
    if !has("access_token")&&!(has("app_key")&&has("refresh_token")) {return Err(invalid("Нужны App key и refresh token либо access token"));}Ok(())
}
fn token(s:&Mutex<Store>)->Result<String> {
    let session=lock(s)?.setting_or("dropbox.session","")?;
    let mut c=lock(s)?.private_get("dropbox")?.ok_or_else(||invalid("Сначала подключите Dropbox"))?;
    let access=c["access_token"].as_str().unwrap_or("");
    if !access.is_empty()&&c["expires"].as_i64().unwrap_or(i64::MAX)>Utc::now().timestamp()+60 {return Ok(access.into());}
    let mut fields=vec![("grant_type","refresh_token"),("refresh_token",c["refresh_token"].as_str().unwrap_or("")),("client_id",c["app_key"].as_str().unwrap_or(""))];
    let secret=c["app_secret"].as_str().unwrap_or("");if !secret.is_empty() {fields.push(("client_secret",secret));}
    let a=body(agent().post("https://api.dropboxapi.com/oauth2/token").send_form(&fields).map_err(error)?)?;
    let access=a["access_token"].as_str().ok_or_else(||invalid("Dropbox не вернул токен"))?.to_string();
    c["access_token"]=json!(access);c["expires"]=json!(Utc::now().timestamp()+a["expires_in"].as_i64().unwrap_or(14400));
    let st=lock(s)?;if st.setting_or("dropbox.session","")?!=session||st.private_get("dropbox")?.is_none() {return Err(invalid("Подключение Dropbox изменилось"));}st.private_put("dropbox",&c)?;Ok(access)
}
fn check_session(s:&Store,session:&str)->Result<()> {if s.setting_or("dropbox.session","")?!=session||s.private_get("dropbox")?.is_none() {return Err(invalid("Dropbox отключён; операция отменена"));}Ok(())}
fn download(t:&str,path:&str)->Result<(String,String)> {
    let r=agent().post(&endpoint(true,"files/download")).set("Authorization",&format!("Bearer {t}")).set("Dropbox-API-Arg",&json!({"path":path}).to_string()).call().map_err(error)?;
    let meta:Value=serde_json::from_str(r.header("Dropbox-API-Result").unwrap_or("{}"))?;
    let rev=meta["rev"].as_str().ok_or_else(||invalid("Dropbox не вернул ревизию"))?.to_string();
    Ok((String::from_utf8(bytes(r,MAX)?).map_err(|_|invalid("Повреждённая общая категория"))?,rev))
}
fn upload(t:&str,path:&str,text:&str,rev:Option<&str>)->Result<bool> {
    if text.len()>MAX {return Err(invalid("Общая категория больше 40 МБ"));}
    let mode=rev.map(|r|json!({".tag":"update","update":r})).unwrap_or(json!("add"));
    match agent().post(&endpoint(true,"files/upload")).set("Authorization",&format!("Bearer {t}")).set("Content-Type","application/octet-stream").set("Dropbox-API-Arg",&json!({"path":path,"mode":mode,"autorename":false,"strict_conflict":true,"mute":true}).to_string()).send_bytes(text.as_bytes()) {
        Ok(_)=>Ok(true),Err(ureq::Error::Status(409,_))=>Ok(false),Err(e)=>Err(error(e))
    }
}
fn remote_id(v:&Value)->Result<&str> {let id=v.as_str().unwrap_or("");if id.is_empty()||id.len()>64||!id.bytes().all(|b|b.is_ascii_digit()) {return Err(invalid("Недопустимый ID общей папки"));}Ok(id)}
fn role(meta:&Value)->Result<&str> {let r=meta["access_type"][".tag"].as_str().unwrap_or("");if !matches!(r,"owner"|"editor"|"viewer"|"viewer_no_comment") {return Err(invalid("Неизвестные права общей папки"));}Ok(r)}
fn path(remote:&str)->String {format!("ns:{remote}/.dayplanner-category.json")}
pub fn run_category(s:&Mutex<Store>,id:&str)->Result<Value> {
    let session=lock(s)?.setting_or("dropbox.session","")?;
    let cat=lock(s)?.shared_get(id)?;
    let remote=cat.remote.ok_or_else(||invalid("Это отключённая локальная копия"))?;
    let t=token(s)?;
    let meta=request(&t,"sharing/get_folder_metadata",&json!({"shared_folder_id":remote}))?;
    let access=role(&meta)?;
    for _ in 0..3 {
        let (text,rev)=download(&t,&path(&remote))?;
        let doc:Document=serde_json::from_str(&text)?; doc.validate()?;
        let merged={let st=lock(s)?;check_session(&st,&session)?;let mut current=st.shared_get(id)?;if current.remote.as_deref()!=Some(&remote) {return Err(invalid("Категория отключена"));}current.document.merge(&doc)?;st.shared_write(id,&current.document)?;st.conn.execute("UPDATE shared_categories SET role=?2 WHERE id=?1",[id,access])?;current.document};
        let writable=matches!(access,"owner"|"editor");
        if !writable||upload(&t,&path(&remote),&serde_json::to_string(&merged)?,Some(&rev))? {
            let st=lock(s)?;check_session(&st,&session)?;
            st.conn.execute("UPDATE shared_categories SET last_ok=?2,error=NULL WHERE id=?1",rusqlite::params![id,Utc::now().timestamp_millis()])?;
            return Ok(json!({"updated":true,"readonly":!writable}));
        }
    }
    Err(invalid("Категорию одновременно меняют. Повторите обновление; локальные правки сохранены"))
}
fn shared_folders(t:&str,m:&str)->Result<Vec<Value>> {
    let mut out=vec![];let mut page=request(t,m,&json!({"limit":100}))?;
    loop {out.extend(page["entries"].as_array().cloned().unwrap_or_default());let Some(cursor)=page["cursor"].as_str() else {break};if out.len()>2000 {return Err(invalid("Слишком много общих папок"));}page=request(t,&format!("{m}/continue"),&json!({"cursor":cursor}))?;}
    Ok(out)
}
pub fn dispatch(s:&Mutex<Store>,cmd:&str,a:&Value)->Result<Value> {
    match cmd {
        "status"=>Ok(json!({"connected":lock(s)?.private_get("dropbox")?.is_some(),"purpose":"shared_categories"})),
        "configure"=>{let c=if let Some(text)=a["text"].as_str() {parse_credentials(text)?} else {validate_credentials(a)?;json!({"app_key":a["app_key"],"refresh_token":a["refresh_token"],"access_token":a["access_token"],"app_secret":a["app_secret"]})};let st=lock(s)?;st.private_put("dropbox",&c)?;st.set_setting("dropbox.session",&sync::hex(&random::<16>()?))?;st.set_setting("dropbox.auto","false")?;Ok(json!({"connected":true}))},
        "disconnect"=>{let st=lock(s)?;st.delete_setting("private.dropbox")?;st.delete_setting("private.dropbox_oauth")?;st.set_setting("dropbox.session",&sync::hex(&random::<16>()?))?;Ok(Value::Null)},
        "check"=>{request(&token(s)?,"sharing/list_folders",&json!({"limit":1}))?;Ok(json!({"name":"Доступ к общим папкам разрешён"}))},
        "create"=>{
            let doc=Document::new(a["name"].as_str().unwrap_or(""))?;let t=token(s)?;let session=lock(s)?.setting_or("dropbox.session","")?;
            let folder=format!("/DayPlanner Shared {}",doc.category);
            request(&t,"files/create_folder_v2",&json!({"path":folder,"autorename":false}))?;
            let meta=request(&t,"sharing/share_folder",&json!({"path":folder,"force_async":false}))?;
            if meta[".tag"]=="async_job_id" {return Err(invalid("Dropbox создаёт папку. Подключите её через список приглашений чуть позже"));}
            let remote=remote_id(&meta["shared_folder_id"])?;
            if !upload(&t,&path(remote),&serde_json::to_string(&doc)?,None)? {return Err(invalid("Файл категории уже существует"));}
            let st=lock(s)?;check_session(&st,&session)?;let id=st.shared_add(remote,"owner",&doc)?;Ok(json!({"id":id}))
        },
        "folders"=>{
            let t=token(s)?;let mut folders=shared_folders(&t,"sharing/list_folders")?;folders.extend(shared_folders(&t,"sharing/list_mountable_folders")?);
            let items:Vec<Value>=folders.iter().map(|v|json!({"id":v["shared_folder_id"],"name":v["name"],"role":v["access_type"][".tag"],"mounted":v["path_lower"].is_string()})).collect();Ok(json!(items))
        },
        "attach"=>{
            let remote=remote_id(&a["remote"])?;let t=token(s)?;let session=lock(s)?.setting_or("dropbox.session","")?;
            let mut meta=request(&t,"sharing/get_folder_metadata",&json!({"shared_folder_id":remote}))?;
            if !meta["path_lower"].is_string() {meta=request(&t,"sharing/mount_folder",&json!({"shared_folder_id":remote}))?;}
            let access=role(&meta)?;
            let (text,_)=download(&t,&path(remote))?;let doc:Document=serde_json::from_str(&text)?;doc.validate()?;
            let st=lock(s)?;check_session(&st,&session)?;let id=st.shared_add(remote,access,&doc)?;Ok(json!({"id":id}))
        },
        "invite"=>{
            let cat=lock(s)?.shared_get(a["id"].as_str().unwrap_or(""))?;
            if cat.role!="owner" {return Err(invalid("Приглашать участников может владелец категории"));}
            let email=a["email"].as_str().unwrap_or("").trim();if email.len()>254||!email.contains('@')||email.chars().any(char::is_whitespace) {return Err(invalid("Введите email участника"));}
            let access=if a["readonly"]==true {"viewer"} else {"editor"};
            request(&token(s)?,"sharing/add_folder_member",&json!({"shared_folder_id":cat.remote.ok_or_else(||invalid("Категория отключена"))?,"members":[{"member":{".tag":"email","email":email},"access_level":access}],"quiet":false}))?;Ok(json!({"invited":true}))
        },
        "category_run"=>run_category(s,a["id"].as_str().unwrap_or("")),
        // Explicitly reject the old whole-database routes, including old UI calls.
        "run"|"backup"|"backups"|"restore"|"config"=>Err(invalid("Dropbox больше не обменивает личную базу. Откройте «Общие категории»")),
        "auth_start"=>{
            let key=a["app_key"].as_str().unwrap_or("");if key.is_empty()||key.len()>100||!key.chars().all(|c|c.is_ascii_alphanumeric()) {return Err(invalid("Введите App key Dropbox"));}
            let verifier=sync::b64_encode(&random::<32>()?).replace('+',"-").replace('/',"_").trim_end_matches('=').to_string();
            let challenge=sync::b64_encode(&Sha256::digest(verifier.as_bytes())).replace('+',"-").replace('/',"_").trim_end_matches('=').to_string();
            lock(s)?.private_put("dropbox_oauth",&json!({"app_key":key,"verifier":verifier,"expires":Utc::now().timestamp()+600}))?;
            Ok(json!({"url":format!("https://www.dropbox.com/oauth2/authorize?client_id={key}&response_type=code&token_access_type=offline&code_challenge_method=S256&code_challenge={challenge}&scope=sharing.read%20sharing.write%20files.metadata.read%20files.content.read%20files.content.write")}))
        },
        "auth_finish"=>{
            let c=lock(s)?.private_get("dropbox_oauth")?.ok_or_else(||invalid("Начните вход заново"))?;if c["expires"].as_i64().unwrap_or(0)<Utc::now().timestamp() {return Err(invalid("Код входа истёк"));}
            let code=a["code"].as_str().filter(|s|!s.is_empty()&&s.len()<4096).ok_or_else(||invalid("Вставьте код входа"))?;
            let ans=body(agent().post("https://api.dropboxapi.com/oauth2/token").send_form(&[("grant_type","authorization_code"),("code",code),("client_id",c["app_key"].as_str().unwrap_or("")),("code_verifier",c["verifier"].as_str().unwrap_or(""))]).map_err(error)?)?;
            let credentials=json!({"app_key":c["app_key"],"refresh_token":ans["refresh_token"],"access_token":ans["access_token"],"expires":Utc::now().timestamp()+ans["expires_in"].as_i64().unwrap_or(14400),"app_secret":""});validate_credentials(&credentials)?;
            let st=lock(s)?;st.private_put("dropbox",&credentials)?;st.delete_setting("private.dropbox_oauth")?;st.set_setting("dropbox.session",&sync::hex(&random::<16>()?))?;Ok(json!({"connected":true}))
        },
        _=>Err(invalid("Неизвестная команда Dropbox"))
    }
}
pub fn start(s:Arc<Mutex<Store>>,changed:Arc<dyn Fn()+Send+Sync>) {
    std::thread::spawn(move||loop {
        std::thread::sleep(Duration::from_secs(60));
        let ids=match lock(&s).and_then(|st| {st.require_unlocked()?;if st.private_get("dropbox")?.is_none() {return Ok(vec![]);}let list=st.shared_list()?;Ok(list.as_array().unwrap().iter().filter(|v|v["remote"].is_string()).filter_map(|v|v["id"].as_str().map(str::to_string)).collect::<Vec<_>>())}) {Ok(ids)=>ids,Err(_)=>continue};
        for id in ids {match run_category(&s,&id) {Ok(_)=>changed(),Err(e)=>{if let Ok(st)=lock(&s) {let _=st.conn.execute("UPDATE shared_categories SET error=?2 WHERE id=?1",rusqlite::params![id,e.to_string()]);}}}}
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{BufRead,BufReader,Write},net::TcpListener};
    #[test]
    fn category_only_transport_retries_revision_conflict() {
        let s=Mutex::new(Store::open_in_memory().unwrap());
        s.lock().unwrap().save_note(&crate::model::Note{title:"PRIVATE MUST NOT UPLOAD".into(),..Default::default()},Utc::now().naive_utc()).unwrap();
        dispatch(&s,"configure",&json!({"access_token":"fake","app_key":"","refresh_token":"","app_secret":""})).unwrap();
        let doc=Document::new("Общая").unwrap();let id=s.lock().unwrap().shared_add("123","editor",&doc).unwrap();
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();
        TEST_ENDPOINT.with(|v|*v.borrow_mut()=Some(format!("http://{}",listener.local_addr().unwrap())));
        let worker=std::thread::spawn(move||{
            for index in 0..5 {
                let (mut stream,_)=listener.accept().unwrap();stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut reader=BufReader::new(stream.try_clone().unwrap());let mut line=String::new();reader.read_line(&mut line).unwrap();let method=line.split_whitespace().nth(1).unwrap().to_string();
                let mut headers=std::collections::HashMap::new();loop {line.clear();reader.read_line(&mut line).unwrap();if line=="\r\n"{break;}let(k,v)=line.split_once(':').unwrap();headers.insert(k.to_lowercase(),v.trim().to_string());}
                let len=headers.get("content-length").and_then(|s|s.parse::<usize>().ok()).unwrap_or(0);let mut data=vec![0;len];reader.read_exact(&mut data).unwrap();
                let (status,text,extra)=if index==0 {assert_eq!(method,"/sharing/get_folder_metadata");(200,json!({"access_type":{".tag":"editor"}}).to_string(),"")}
                else if index%2==1 {assert_eq!(method,"/files/download");assert!(headers["dropbox-api-arg"].contains("ns:123/.dayplanner-category.json"));(200,serde_json::to_string(&doc).unwrap(),"Dropbox-API-Result: {\"rev\":\"r1\"}\r\n")}
                else {assert_eq!(method,"/files/upload");let text=String::from_utf8(data).unwrap();assert!(!text.contains("PRIVATE MUST NOT UPLOAD"));let parsed:Document=serde_json::from_str(&text).unwrap();assert_eq!(parsed.category,doc.category);assert!(headers["dropbox-api-arg"].contains("update"));(if index==2{409}else{200},"{}".into(),"")};
                write!(stream,"HTTP/1.1 {status} Test\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n{text}",text.len()).unwrap();
            }
        });
        let result=run_category(&s,&id);TEST_ENDPOINT.with(|v|*v.borrow_mut()=None);result.unwrap();worker.join().unwrap();
        assert_eq!(s.lock().unwrap().list_notes(None,None).unwrap()[0].title,"PRIVATE MUST NOT UPLOAD");
    }
}
