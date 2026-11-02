//! Передача данных синхронизации между устройствами.
//!
//! Два способа, оба без собственного сервера:
//! * **ntfy** — бесплатный публичный сервис сообщений (https://ntfy.sh, можно указать свой). Устройства группы
//!   читают и пишут «канал», имя которого выводится из кода группы. Содержимое зашифровано кодом,
//!   поэтому сервис видит только бессмысленные байты.
//! * **Локальная сеть** — устройства одной группы находят друг друга широковещательным UDP-сообщением и
//!   обмениваются данными напрямую по TCP, тоже в зашифованном виде.
//!
//! Всё это работает в фоновых потоках, запускаемых `start`. Хранилище блокируется только на время
//! чтения и записи, но не на время сетевого запроса.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::store::Store;
use crate::sync::{
    b64_decode, b64_encode, derive_keys, open, random_hex, seal, GroupKeys, MergeStats, Payload, CHANGES,
};

pub const LAN_TCP_PORT: u16 = 47831;
pub const LAN_UDP_PORT: u16 = 47832;
/// Размер одного сообщения ntfy в символах base64 (лимит сервиса — 4096 байт).
const CHUNK_CHARS: usize = 3000;
/// Больше такого числа сообщений за один раз не отправляем: у бесплатного ntfy.sh есть лимиты.
const MAX_CHUNKS: usize = 120;
/// Записей в одном пакете при отправке через ntfy.
const BATCH_RECORDS: usize = 120;
const FULL_EVERY_MS: i64 = 6 * 3600 * 1000;
const POLL_EVERY: Duration = Duration::from_secs(60);
const MAX_FRAME: usize = 32 * 1024 * 1024;

static ON_CHANGE: OnceLock<Arc<dyn Fn() + Send + Sync>> = OnceLock::new();
static WAKE: AtomicBool = AtomicBool::new(false);
static LAN_NOW: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);
static LAN_PORT: AtomicU16 = AtomicU16::new(0);

fn lock(store: &Mutex<Store>) -> MutexGuard<'_, Store> {
    store.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn net(msg: impl Into<String>) -> Error {
    Error::Net(msg.into())
}

/// Сообщает интерфейсу, что данные изменились из-за синхронизации.
pub fn notify_changed() {
    if let Some(f) = ON_CHANGE.get() {
        f();
    }
}

/// Просит фоновую синхронизацию выполниться как можно скорее.
pub fn wake() {
    WAKE.store(true, Ordering::Relaxed);
    LAN_NOW.store(true, Ordering::Relaxed);
}

/// Адрес этого устройства в локальной сети (для ввода вручную на другом устройстве).
pub fn local_address() -> Option<String> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    // Для UDP connect ничего не отправляет: система лишь выбирает подходящий адрес интерфейса.
    sock.connect("192.0.2.1:9").ok()?;
    let ip = sock.local_addr().ok()?.ip();
    if ip.is_unspecified() || ip.is_loopback() {
        return None;
    }
    let port = LAN_PORT.load(Ordering::Relaxed);
    Some(format!("{}:{}", ip, if port == 0 { LAN_TCP_PORT } else { port }))
}

fn record_ok(store: &Mutex<Store>, lan: bool) {
    let s = lock(store);
    let ms = now_ms().to_string();
    let _ = s.set_setting("sync.last_ok", &ms);
    if lan {
        let _ = s.set_setting("sync.last_lan", &ms);
    }
    let _ = s.delete_setting("sync.error");
}

fn record_error(store: &Mutex<Store>, e: &Error) {
    let _ = lock(store).set_setting("sync.error", &e.to_string());
}

// ---------- ntfy ----------

fn http_error(e: ureq::Error) -> Error {
    match e {
        ureq::Error::Status(429, _) => net("Сервис ntfy временно ограничил число сообщений: повторите позже"),
        ureq::Error::Status(code, _) => net(format!("сервер ntfy ответил кодом {code}")),
        ureq::Error::Transport(t) => net(format!("нет связи с сервером ntfy: {t}")),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(25)).build()
}

/// Разбивает зашифрованный снимок на сообщения: `DP1.<id>.<номер>.<всего>.<base64>`.
fn to_messages(keys: &GroupKeys, payload: &Payload) -> Result<Vec<String>> {
    let json = serde_json::to_string(payload)?;
    let sealed = seal(&keys.key, json.as_bytes())?;
    let b64 = b64_encode(&sealed);
    let pieces: Vec<&str> = b64
        .as_bytes()
        .chunks(CHUNK_CHARS)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    let mid = random_hex(6);
    let total = pieces.len();
    Ok(pieces.iter().enumerate().map(|(i, p)| format!("DP1.{mid}.{}.{total}.{p}", i + 1)).collect())
}

struct Incoming {
    total: usize,
    parts: HashMap<usize, String>,
}

/// Собирает сообщения обратно в снимки. Неполные наборы пропускаются: они дозреют при следующем опросе.
fn assemble(messages: &[String]) -> Vec<(String, String)> {
    let mut sets: HashMap<String, Incoming> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for m in messages {
        let mut it = m.trim().splitn(5, '.');
        if it.next() != Some("DP1") {
            continue;
        }
        let (Some(mid), Some(idx), Some(total), Some(body)) = (it.next(), it.next(), it.next(), it.next()) else {
            continue;
        };
        let (Ok(idx), Ok(total)) = (idx.parse::<usize>(), total.parse::<usize>()) else {
            continue;
        };
        if total == 0 || total > 200 || idx == 0 || idx > total {
            continue;
        }
        if !sets.contains_key(mid) {
            order.push(mid.to_string());
        }
        let e = sets.entry(mid.to_string()).or_insert(Incoming { total, parts: HashMap::new() });
        if e.total == total {
            e.parts.insert(idx, body.to_string());
        }
    }
    let mut out = Vec::new();
    for mid in order {
        if let Some(e) = sets.get(&mid) {
            if e.parts.len() == e.total {
                let mut joined = String::new();
                for i in 1..=e.total {
                    joined.push_str(&e.parts[&i]);
                }
                out.push((mid, joined));
            }
        }
    }
    out
}

/// Один цикл обмена через ntfy: забрать чужое, слить, отправить своё.
fn ntfy_cycle(store: &Mutex<Store>) -> Result<(MergeStats, usize)> {
    let (cfg, since, pushed, hello, seen) = {
        let s = lock(store);
        let cfg = s.sync_config()?;
        (
            cfg,
            s.get_setting("sync.since")?,
            s.get_setting("sync.pushed")?,
            s.get_setting("sync.hello")?.is_some(),
            s.get_setting("sync.seen")?.unwrap_or_default(),
        )
    };
    let Some(code) = cfg.code.clone() else {
        return Err(Error::Invalid("Группа синхронизации не настроена".into()));
    };
    let keys = derive_keys(&code);
    let base = cfg.server.trim_end_matches('/').to_string();
    let http = agent();

    // 1. Забрать сообщения группы.
    let url = format!("{base}/{}/json?poll=1&since={}", keys.topic, since.clone().unwrap_or_else(|| "12h".into()));
    let body = http.get(&url).call().map_err(http_error)?.into_string().map_err(|e| net(e.to_string()))?;
    let mut texts: Vec<String> = Vec::new();
    let mut newest: i64 = 0;
    for line in body.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v["event"] != "message" {
            continue;
        }
        if let Some(t) = v["time"].as_i64() {
            newest = newest.max(t);
        }
        if let Some(m) = v["message"].as_str() {
            texts.push(m.to_string());
        }
    }
    let mut seen_ids: Vec<String> = seen.split(',').filter(|x| !x.is_empty()).map(|x| x.to_string()).collect();
    let mut merged = MergeStats::default();
    let mut peer_said_hello = false;
    for (mid, joined) in assemble(&texts) {
        if seen_ids.contains(&mid) {
            continue;
        }
        seen_ids.push(mid);
        let Ok(raw) = b64_decode(&joined) else { continue };
        let Ok(plain) = open(&keys.key, &raw) else { continue };
        let Ok(p) = serde_json::from_slice::<Payload>(&plain) else { continue };
        if p.device == cfg.device {
            continue;
        }
        if p.msg == "hello" {
            peer_said_hello = true;
        }
        // Снимок от более новой версии или повреждённый пропускаем: он не должен блокировать группу.
        match lock(store).apply_payload(&p) {
            Ok(stats) => merged.add(&stats),
            Err(Error::Invalid(_)) => continue,
            Err(e) => return Err(e),
        }
    }
    while seen_ids.len() > 300 {
        seen_ids.remove(0);
    }
    {
        let s = lock(store);
        s.set_setting("sync.seen", &seen_ids.join(","))?;
        if newest > 0 {
            // Небольшой запас назад: сообщения, пришедшие частями, успеют собраться.
            s.set_setting("sync.since", &(newest - 60).max(0).to_string())?;
        }
    }
    if merged.applied > 0 {
        notify_changed();
    }

    // 2. Отправить своё.
    let mark: i64 = pushed.as_deref().and_then(|x| x.parse().ok()).unwrap_or(0);
    // Полный снимок раз в 6 часов: ntfy хранит сообщения около 12 часов, и долго бывшее без связи устройство иначе
    // пропустило бы чужие правки.
    let full_at: i64 = lock(store).get_setting("sync.full_at")?.and_then(|x| x.parse().ok()).unwrap_or(0);
    let full = hello || peer_said_hello || pushed.is_none() || now_ms() - full_at > FULL_EVERY_MS;
    let (payload, high_water) = {
        let s = lock(store);
        let high = s.sync_high_water();
        (s.make_payload(if full { 0 } else { mark }, "data")?, high)
    };
    let mut sent = 0usize;
    if full || !payload.records.is_empty() {
        let mut messages: Vec<String> = Vec::new();
        if hello {
            let hello_payload = Payload { v: 1, device: cfg.device.clone(), msg: "hello".into(), records: vec![], unlocked: vec![] };
            messages.extend(to_messages(&keys, &hello_payload)?);
        }
        // Большой снимок отправляем несколькими пакетами: каждый применяется независимо.
        let empty: &[crate::sync::Record] = &[];
        let batches: Vec<&[crate::sync::Record]> = if payload.records.is_empty() {
            vec![empty]
        } else {
            payload.records.chunks(BATCH_RECORDS).collect()
        };
        for (i, b) in batches.iter().enumerate() {
            let part = Payload {
                v: payload.v,
                device: payload.device.clone(),
                msg: "data".into(),
                records: b.to_vec(),
                unlocked: if i == 0 { payload.unlocked.clone() } else { vec![] },
            };
            messages.extend(to_messages(&keys, &part)?);
        }
        if messages.len() > MAX_CHUNKS {
            return Err(net("Слишком много данных для обмена через ntfy: передайте их по Wi-Fi или файлом"));
        }
        let url = format!("{base}/{}", keys.topic);
        for m in &messages {
            http.post(&url)
                .set("Content-Type", "text/plain; charset=utf-8")
                .send_string(m)
                .map_err(http_error)?;
        }
        sent = payload.records.len();
        let s = lock(store);
        s.set_setting("sync.pushed", &high_water.to_string())?;
        if full {
            s.set_setting("sync.full_at", &now_ms().to_string())?;
        }
        if hello {
            s.delete_setting("sync.hello")?;
        }
    }
    Ok((merged, sent))
}

// ---------- локальная сеть ----------

fn write_frame(stream: &mut TcpStream, data: &[u8]) -> Result<()> {
    let len = (data.len() as u32).to_be_bytes();
    stream.write_all(&len).and_then(|_| stream.write_all(data)).and_then(|_| stream.flush()).map_err(|e| net(e.to_string()))
}

fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut len = [0u8; 4];
    stream.read_exact(&mut len).map_err(|e| net(e.to_string()))?;
    let n = u32::from_be_bytes(len) as usize;
    if n == 0 || n > MAX_FRAME {
        return Err(net("некорректный пакет"));
    }
    let mut buf = vec![0u8; n];
    stream.read_exact(&mut buf).map_err(|e| net(e.to_string()))?;
    Ok(buf)
}

fn snapshot_frame(store: &Mutex<Store>, keys: &GroupKeys) -> Result<Vec<u8>> {
    let p = lock(store).make_payload(0, "data")?;
    seal(&keys.key, serde_json::to_string(&p)?.as_bytes())
}

fn apply_frame(store: &Mutex<Store>, keys: &GroupKeys, frame: &[u8]) -> Result<MergeStats> {
    let plain = open(&keys.key, frame)?;
    let p: Payload = serde_json::from_slice(&plain).map_err(|_| Error::Invalid("повреждённые данные синхронизации".into()))?;
    let stats = lock(store).apply_payload(&p)?;
    if stats.applied > 0 {
        notify_changed();
    }
    Ok(stats)
}

/// Обмен «вы мне — я вам» с устройством по адресу.
fn exchange(store: &Mutex<Store>, keys: &GroupKeys, addr: SocketAddr) -> Result<MergeStats> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|e| net(format!("не удалось подключиться к {addr}: {e}")))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(20)));
    write_frame(&mut stream, &snapshot_frame(store, keys)?)?;
    let reply = read_frame(&mut stream)?;
    let stats = apply_frame(store, keys, &reply)?;
    record_ok(store, true);
    Ok(stats)
}

fn serve(store: &Mutex<Store>, keys: &GroupKeys, mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(20)));
    let Ok(frame) = read_frame(&mut stream) else { return };
    // Чужой код или мусор: молча закрываем соединение.
    if apply_frame(store, keys, &frame).is_err() {
        return;
    }
    if let Ok(reply) = snapshot_frame(store, keys) {
        let _ = write_frame(&mut stream, &reply);
    }
    record_ok(store, true);
}

/// Ручное подключение по адресу `ip` или `ip:порт`.
pub fn connect_addr(store: &Mutex<Store>, addr: &str) -> Result<Value> {
    let code = lock(store).sync_config()?.code.ok_or_else(|| Error::Invalid("Сначала создайте группу или введите код".into()))?;
    let keys = derive_keys(&code);
    let text = addr.trim();
    if text.is_empty() {
        return Err(Error::Invalid("Введите адрес устройства, например 192.168.1.5".into()));
    }
    let with_port = if text.contains(':') { text.to_string() } else { format!("{text}:{LAN_TCP_PORT}") };
    let target = with_port
        .to_socket_addrs()
        .map_err(|_| Error::Invalid("Неверный адрес устройства".into()))?
        .next()
        .ok_or_else(|| Error::Invalid("Неверный адрес устройства".into()))?;
    let stats = exchange(store, &keys, target).map_err(|e| match e {
        Error::Invalid(_) => net("Устройство ответило, но код группы не совпал"),
        other => other,
    })?;
    Ok(json!({ "applied": stats.applied }))
}

fn beacon_text(keys: &GroupKeys, device: &str, port: u16) -> String {
    format!("DPLAN1 {} {} {}", keys.lan, device, port)
}

struct Peer {
    addr: SocketAddr,
    seen: Instant,
    /// Когда в последний раз обменивались; `None` — ещё ни разу.
    synced: Option<Instant>,
}

/// Поток локальной сети: слушает TCP, рассылает и принимает маячки, обменивается с найденными устройствами.
fn lan_loop(store: Arc<Mutex<Store>>) {
    let mut listener: Option<TcpListener> = None;
    let mut udp: Option<UdpSocket> = None;
    let mut conf: Option<(GroupKeys, String)> = None;
    // `None` — «пора»: вычитать длительность из Instant нельзя, сразу после загрузки системы это паникует.
    let mut conf_at: Option<Instant> = None;
    let mut last_beacon: Option<Instant> = None;
    let mut peers: HashMap<String, Peer> = HashMap::new();
    let mut seen_changes = CHANGES.load(Ordering::Relaxed);
    let mut dirty_since: Option<Instant> = None;
    let mut port: u16 = 0;
    let mut buf = [0u8; 256];

    loop {
        thread::sleep(Duration::from_millis(250));
        let now = Instant::now();

        if conf_at.map(|t| now.duration_since(t) >= Duration::from_secs(3)).unwrap_or(true) {
            conf_at = Some(now);
            let cfg = lock(&store).sync_config();
            conf = match cfg {
                Ok(c) if c.lan && c.code.is_some() => c.code.map(|code| (derive_keys(&code), c.device)),
                _ => None,
            };
            if conf.is_none() {
                if listener.is_some() || udp.is_some() {
                    listener = None;
                    udp = None;
                    peers.clear();
                    LAN_PORT.store(0, Ordering::Relaxed);
                }
            } else {
                if listener.is_none() {
                    for p in LAN_TCP_PORT..LAN_TCP_PORT + 6 {
                        if let Ok(l) = TcpListener::bind(("0.0.0.0", p)) {
                            let _ = l.set_nonblocking(true);
                            port = p;
                            LAN_PORT.store(p, Ordering::Relaxed);
                            listener = Some(l);
                            break;
                        }
                    }
                }
                if udp.is_none() {
                    if let Ok(u) = UdpSocket::bind(("0.0.0.0", LAN_UDP_PORT)) {
                        let _ = u.set_broadcast(true);
                        let _ = u.set_nonblocking(true);
                        udp = Some(u);
                    }
                }
            }
        }
        let Some((keys, device)) = conf.clone() else {
            LAN_NOW.store(false, Ordering::Relaxed);
            continue;
        };

        // Входящие подключения.
        if let Some(l) = &listener {
            while let Ok((stream, _)) = l.accept() {
                let _ = stream.set_nonblocking(false);
                let st = store.clone();
                let k = keys.clone();
                thread::spawn(move || serve(&st, &k, stream));
            }
        }

        // Маячки: слушаем чужие, раз в 15 секунд (или по просьбе) рассылаем свой.
        if let Some(u) = &udp {
            while let Ok((n, from)) = u.recv_from(&mut buf) {
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let parts: Vec<&str> = text.split_whitespace().collect();
                if parts.len() == 4 && parts[0] == "DPLAN1" && parts[1] == keys.lan && parts[2] != device {
                    if let Ok(p) = parts[3].parse::<u16>() {
                        let addr = SocketAddr::new(from.ip(), p);
                        let entry = peers.entry(parts[2].to_string()).or_insert(Peer { addr, seen: now, synced: None });
                        entry.addr = addr;
                        entry.seen = now;
                    }
                }
            }
            let want_now = LAN_NOW.swap(false, Ordering::Relaxed);
            if listener.is_some() && (want_now || last_beacon.map(|t| now.duration_since(t) >= Duration::from_secs(15)).unwrap_or(true)) {
                last_beacon = Some(now);
                let msg = beacon_text(&keys, &device, port);
                let _ = u.send_to(msg.as_bytes(), ("255.255.255.255", LAN_UDP_PORT));
            }
        }

        // Обмен с найденными устройствами: при первой встрече, раз в две минуты и вскоре после локальных правок.
        let ch = CHANGES.load(Ordering::Relaxed);
        if ch != seen_changes {
            seen_changes = ch;
            dirty_since = Some(now);
        }
        let dirty = dirty_since.map(|d| now.duration_since(d) >= Duration::from_secs(3)).unwrap_or(false);
        let mut due: Vec<(String, SocketAddr)> = Vec::new();
        peers.retain(|_, p| now.duration_since(p.seen) < Duration::from_secs(120));
        for (id, p) in peers.iter() {
            let age = p.synced.map(|t| now.duration_since(t)).unwrap_or(Duration::MAX);
            let fresh = now.duration_since(p.seen) < Duration::from_secs(45);
            if age >= Duration::from_secs(120) || (dirty && fresh && age >= Duration::from_secs(3)) {
                due.push((id.clone(), p.addr));
            }
        }
        if !due.is_empty() {
            dirty_since = None;
        }
        for (id, addr) in due {
            if let Some(p) = peers.get_mut(&id) {
                p.synced = Some(Instant::now());
            }
            // Ошибка одного устройства не должна мешать остальным; повтор будет позже.
            let _ = exchange(&store, &keys, addr);
        }
    }
}

// ---------- ручной запуск и фон ----------

/// «Синхронизировать сейчас»: цикл ntfy и просьба локальной сети поискать устройства.
pub fn run_now(store: &Mutex<Store>) -> Result<Value> {
    let cfg = lock(store).sync_config()?;
    if cfg.code.is_none() {
        return Err(Error::Invalid("Сначала создайте группу или введите код".into()));
    }
    if !cfg.ntfy && !cfg.lan {
        return Err(Error::Invalid("Включите хотя бы один способ: через интернет или по локальной сети".into()));
    }
    LAN_NOW.store(true, Ordering::Relaxed);
    let mut applied = 0;
    let mut pushed = 0;
    if cfg.ntfy {
        match ntfy_cycle(store) {
            Ok((m, p)) => {
                applied = m.applied;
                pushed = p;
                record_ok(store, false);
            }
            Err(e) => {
                record_error(store, &e);
                return Err(e);
            }
        }
    }
    Ok(json!({ "applied": applied, "pushed": pushed }))
}

/// Запускает фоновую синхронизацию. `on_change` вызывается, когда из сети пришли новые данные.
pub fn start(store: Arc<Mutex<Store>>, on_change: Arc<dyn Fn() + Send + Sync>) {
    crate::pairing::start(store.clone());
    let _ = ON_CHANGE.set(on_change);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let lan_store = store.clone();
    thread::spawn(move || lan_loop(lan_store));
    thread::spawn(move || {
        let mut last_run: Option<Instant> = None;
        let mut extra_at: Option<Instant> = None;
        let mut seen_changes = CHANGES.load(Ordering::Relaxed);
        let mut dirty_since: Option<Instant> = None;
        loop {
            thread::sleep(Duration::from_secs(2));
            let now = Instant::now();
            let ch = CHANGES.load(Ordering::Relaxed);
            if ch != seen_changes {
                seen_changes = ch;
                dirty_since = Some(now);
            }
            let woke = WAKE.swap(false, Ordering::Relaxed);
            let due = last_run.map(|t| now.duration_since(t) >= POLL_EVERY).unwrap_or(true);
            let dirty = dirty_since.map(|d| now.duration_since(d) >= Duration::from_secs(3)).unwrap_or(false);
            let extra = extra_at.map(|t| now >= t).unwrap_or(false);
            if !(woke || due || dirty || extra) {
                continue;
            }
            last_run = Some(now);
            dirty_since = None;
            extra_at = None;
            let cfg = match lock(&store).sync_config() {
                Ok(c) => c,
                Err(_) => continue,
            };
            if cfg.code.is_none() || !cfg.ntfy {
                continue;
            }
            let had_hello = lock(&store).get_setting("sync.hello").ok().flatten().is_some();
            match ntfy_cycle(&store) {
                Ok(_) => {
                    record_ok(&store, false);
                    if had_hello {
                        // Только что вступили в группу: через несколько секунд заберём ответы остальных.
                        extra_at = Some(Instant::now() + Duration::from_secs(12));
                    }
                }
                Err(e) => record_error(&store, &e),
            }
        }
    });
}
