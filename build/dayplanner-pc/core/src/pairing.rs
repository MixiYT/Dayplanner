//! On-demand LAN discovery; ephemeral X25519, 8-digit SAS and both-screen approval.
//! Discovery is untrusted. No group secret is sent until both users approve the SAS.
use std::{io::{Read,Write},net::{TcpStream,TcpListener,UdpSocket,SocketAddr},sync::{Arc,Mutex,OnceLock},time::{Duration,Instant}};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use x25519_dalek::{StaticSecret,PublicKey};
use crate::{Store,Result,features::{invalid,random},sync};
const UDP:u16=47842;
static STORE:OnceLock<Arc<Mutex<Store>>>=OnceLock::new();
static STATE:OnceLock<Mutex<State>>=OnceLock::new();
#[derive(Default)]
struct State {mode:String,session:String,sas:String,approved:bool,peer:String,error:String,until:Option<Instant>,attempts:u8,busy:bool}
fn state()->std::sync::MutexGuard<'static,State> {STATE.get_or_init(||Mutex::new(State::default())).lock().unwrap_or_else(|e|e.into_inner())}
fn active(s:&State)->bool {s.until.is_some_and(|t|Instant::now()<t)}
fn frame(s:&mut TcpStream,v:&Value)->Result<()> {let b=serde_json::to_vec(v)?;if b.len()>8192 {return Err(invalid("Слишком большой пакет подключения"));}s.write_all(&(b.len() as u32).to_be_bytes()).and_then(|_|s.write_all(&b)).map_err(|_|invalid("Связь с устройством прервана"))}
fn recv(s:&mut TcpStream)->Result<Value> {let mut n=[0;4];s.read_exact(&mut n).map_err(|_|invalid("Нет ответа устройства"))?;let n=u32::from_be_bytes(n) as usize;if n>8192 {return Err(invalid("Неверный пакет подключения"));}let mut b=vec![0;n];s.read_exact(&mut b).map_err(|_|invalid("Связь прервана"))?;Ok(serde_json::from_slice(&b)?)}
fn public(v:&Value)->Result<PublicKey> {let b:[u8;32]=sync::b64_decode(v.as_str().unwrap_or(""))?.try_into().map_err(|_|invalid("Неверный ключ устройства"))?;Ok(PublicKey::from(b))}
fn keys(secret:&StaticSecret,peer:&PublicKey,host:&PublicKey,client:&PublicKey,session:&str)->Result<([u8;32],String)> {
    let dh=secret.diffie_hellman(peer);if dh.as_bytes()==&[0;32] {return Err(invalid("Недопустимый ключ устройства"));}
    let mut h=Sha256::new();h.update(b"DayPlanner LAN pairing v1");h.update(dh.as_bytes());h.update(host.as_bytes());h.update(client.as_bytes());h.update(session.as_bytes());let key:[u8;32]=h.finalize().into();
    let mut h=Sha256::new();h.update(b"DayPlanner SAS v1");h.update(key);let digest=h.finalize();let sas=format!("{:08}",u32::from_be_bytes(digest[..4].try_into().unwrap())%100_000_000);Ok((key,sas))
}
fn encrypted(key:&[u8;32],v:&Value)->Result<Value> {Ok(json!({"sealed":sync::b64_encode(&sync::seal(key,&serde_json::to_vec(v)?)?)}))}
fn decrypt(key:&[u8;32],v:&Value)->Result<Value> {Ok(serde_json::from_slice(&sync::open(key,&sync::b64_decode(v["sealed"].as_str().unwrap_or(""))?)?)?)}
fn identity()->String {if cfg!(target_os="android") {"Телефон Android".into()} else {"Компьютер".into()}}
fn set_sas(session:&str,sas:&str,peer:&str)->Result<()> {let mut st=state();if st.session!=session||!active(&st) {return Err(invalid("Подключение отменено"));}st.sas=sas.into();st.peer=peer.chars().take(60).collect();st.approved=false;Ok(())}
fn approved(session:&str)->Result<bool> {let st=state();if st.session!=session||!active(&st) {return Err(invalid("Время подключения истекло или оно отменено"));}Ok(st.approved)}
fn finish(session:&str,result:Result<()>) {let mut st=state();if st.session==session {st.busy=false;st.approved=false;st.sas.clear();match result {Ok(())=>{st.mode="done".into();st.until=None;},Err(e)=>{st.mode="error".into();st.error=e.to_string();st.until=None;}}}}
fn server(mut stream:TcpStream,session:String)->Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();stream.set_write_timeout(Some(Duration::from_secs(4))).ok();
    let secret=StaticSecret::from(random::<32>()?);let host=PublicKey::from(&secret);
    frame(&mut stream,&json!({"v":1,"session":session,"public":sync::b64_encode(host.as_bytes()),"name":identity()}))?;
    let hello=recv(&mut stream)?;let client=public(&hello["public"])?;
    let (key,sas)=keys(&secret,&client,&host,&client,&session)?;
    set_sas(&session,&sas,hello["name"].as_str().unwrap_or("Устройство"))?;
    // Client sends authenticated polling frames only after computing the same key.
    loop {
        let v=decrypt(&key,&recv(&mut stream)?)?;
        if v["confirm"]==true&&approved(&session)? {break;}
        if !active(&state()) {return Err(invalid("Подключение отменено"));}
        frame(&mut stream,&encrypted(&key,&json!({"waiting":true}))?)?;
    }
    let store=STORE.get().ok_or_else(||invalid("LAN ещё запускается"))?;
    let st=store.lock().map_err(|_|invalid("Хранилище недоступно"))?;st.require_unlocked()?;
    let code=st.sync_config()?.code.ok_or_else(||invalid("Личная группа отключена"))?;
    frame(&mut stream,&encrypted(&key,&json!({"code":code}))?)?;
    Ok(())
}
fn client(addr:SocketAddr,session:String)->Result<()> {
    let mut stream=TcpStream::connect_timeout(&addr,Duration::from_secs(5)).map_err(|_|invalid("Устройство недоступно. Проверьте Wi-Fi и брандмауэр"))?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let hello=recv(&mut stream)?;if hello["session"]!=session||hello["v"]!=1 {return Err(invalid("Предложение подключения изменилось"));}
    let host=public(&hello["public"])?;let secret=StaticSecret::from(random::<32>()?);let client=PublicKey::from(&secret);
    frame(&mut stream,&json!({"public":sync::b64_encode(client.as_bytes()),"name":identity()}))?;
    let (key,sas)=keys(&secret,&host,&host,&client,&session)?;set_sas(&session,&sas,hello["name"].as_str().unwrap_or("Устройство"))?;
    loop {
        let yes=approved(&session)?;frame(&mut stream,&encrypted(&key,&json!({"confirm":yes}))?)?;
        let v=decrypt(&key,&recv(&mut stream)?)?;
        if let Some(code)=v["code"].as_str() {
            if !yes {return Err(invalid("Устройство прислало код до подтверждения"));}
            let store=STORE.get().ok_or_else(||invalid("LAN ещё запускается"))?;let st=store.lock().map_err(|_|invalid("Хранилище недоступно"))?;st.require_unlocked()?;
            // Recheck after the network response; Cancel must not join a group.
            if !approved(&session)? {return Err(invalid("Подключение отменено"));}
            st.sync_join_group(code)?;st.sync_set_transports(false,true,crate::sync::DEFAULT_NTFY_SERVER)?;crate::sync_net::wake();return Ok(());
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}
pub fn start(store:Arc<Mutex<Store>>) {
    if STORE.set(store).is_err() {return;}
    std::thread::spawn(|| {
        let mut udp=None;let mut tcp=None;
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let (on,session)={let st=state();(st.mode=="host"&&active(&st),st.session.clone())};
            if !on {udp=None;tcp=None;continue;}
            if tcp.is_none() {tcp=(47841..47847).find_map(|p|TcpListener::bind(("0.0.0.0",p)).ok());if let Some(t)=&tcp {t.set_nonblocking(true).ok();}}
            if udp.is_none() {udp=UdpSocket::bind(("0.0.0.0",UDP)).ok();if let Some(u)=&udp {u.set_nonblocking(true).ok();}}
            let (Some(u),Some(t))=(&udp,&tcp) else {let mut st=state();st.error="Не удалось открыть LAN-порты 47841–47846/47842. Проверьте брандмауэр".into();continue};
            let mut b=[0;256];while let Ok((n,from))=u.recv_from(&mut b) {if &b[..n]==b"DPLAN-PAIR-DISCOVER-1" {let msg=json!({"v":1,"session":session,"port":t.local_addr().unwrap().port(),"name":identity()}).to_string();let _=u.send_to(msg.as_bytes(),from);}}
            while let Ok((stream,_))=t.accept() {let mut st=state();if st.busy||st.attempts>=5 {continue;}st.busy=true;st.attempts+=1;drop(st);let id=session.clone();std::thread::spawn(move||{let result=server(stream,id.clone());finish(&id,result);});}
        }
    });
}
pub fn dispatch(store:&Mutex<Store>,cmd:&str,a:&Value)->Result<Value> {
    match cmd {
        "host"=>{let st=store.lock().map_err(|_|invalid("Хранилище недоступно"))?;if st.sync_config()?.code.is_none() {st.sync_create_group()?;st.sync_set_transports(false,true,crate::sync::DEFAULT_NTFY_SERVER)?;}let mut s=state();*s=State{mode:"host".into(),session:sync::hex(&random::<16>()?),until:Some(Instant::now()+Duration::from_secs(180)),..Default::default()};Ok(json!({"started":true}))},
        "status"=>{let s=state();Ok(json!({"mode":s.mode,"sas":s.sas,"peer":s.peer,"error":s.error,"approved":s.approved,"remaining":s.until.map(|t|t.saturating_duration_since(Instant::now()).as_secs()).unwrap_or(0)}))},
        "cancel"=>{*state()=State::default();Ok(Value::Null)},
        "confirm"=>{let mut s=state();if !active(&s)||s.sas.is_empty()||a["sas"].as_str()!=Some(&s.sas) {return Err(invalid("Код изменился или истёк"));}s.approved=true;Ok(Value::Null)},
        "find"=>{
            let u=UdpSocket::bind("0.0.0.0:0").map_err(|_|invalid("Нет доступа к локальной сети"))?;u.set_broadcast(true).ok();u.set_read_timeout(Some(Duration::from_millis(300))).ok();
            let mut devices=std::collections::BTreeMap::new();let end=Instant::now()+Duration::from_secs(3);let mut b=[0;512];
            while Instant::now()<end {let _=u.send_to(b"DPLAN-PAIR-DISCOVER-1",("255.255.255.255",UDP));if let Ok((n,from))=u.recv_from(&mut b) {if let Ok(v)=serde_json::from_slice::<Value>(&b[..n]) {let session=v["session"].as_str().unwrap_or("");let port=v["port"].as_u64().unwrap_or(0);if v["v"]==1&&session.len()==32&&(47841..47847).contains(&port) {devices.insert(session.to_string(),json!({"session":session,"addr":SocketAddr::new(from.ip(),port as u16).to_string(),"name":v["name"].as_str().unwrap_or("Устройство").chars().take(60).collect::<String>()}));}}}}
            Ok(json!(devices.values().collect::<Vec<_>>()))
        },
        "connect"=>{
            let addr:SocketAddr=a["addr"].as_str().unwrap_or("").parse().map_err(|_|invalid("Неверный адрес устройства"))?;
            if !(47841..47847).contains(&addr.port())||!matches!(addr.ip(),std::net::IpAddr::V4(ip) if ip.is_private()||ip.is_loopback()||ip.is_link_local()) {return Err(invalid("Допустим только локальный адрес"));}
            let session=a["session"].as_str().unwrap_or("").to_string();if session.len()!=32||!session.bytes().all(|b|b.is_ascii_hexdigit()) {return Err(invalid("Неверное предложение устройства"));}
            {let mut s=state();*s=State{mode:"client".into(),session:session.clone(),until:Some(Instant::now()+Duration::from_secs(180)),busy:true,..Default::default()};}
            std::thread::spawn(move||{let r=client(addr,session.clone());finish(&session,r);});Ok(Value::Null)
        },
        _=>Err(invalid("Неизвестная команда подключения"))
    }
}
#[cfg(test)] mod tests {use super::*;#[test]fn sas_agrees_and_transcript_is_bound(){let a=StaticSecret::from([1;32]);let b=StaticSecret::from([2;32]);let ap=PublicKey::from(&a);let bp=PublicKey::from(&b);let (ka,sa)=keys(&a,&bp,&ap,&bp,"session").unwrap();let (kb,sb)=keys(&b,&ap,&ap,&bp,"session").unwrap();assert_eq!((ka,sa),(kb,sb));assert_ne!(ka,keys(&a,&bp,&ap,&bp,"other").unwrap().0);assert!(keys(&a,&PublicKey::from([0;32]),&ap,&bp,"session").is_err());assert_eq!(decrypt(&ka,&encrypted(&ka,&json!({"confirm":true})).unwrap()).unwrap()["confirm"],true);}}
