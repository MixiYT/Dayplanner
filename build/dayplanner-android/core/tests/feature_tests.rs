use std::io::{Cursor, Write};
use std::sync::Mutex;
use chrono::NaiveDateTime;
use serde_json::{json, Value};
use planner_core::{api::dispatch, Store, model::Note, sync::{b64_encode_for_tests as b64, derive_keys}, features, dropbox::parse_credentials};

fn now() -> NaiveDateTime { NaiveDateTime::parse_from_str("2026-10-30T12:00:00","%Y-%m-%dT%H:%M:%S").unwrap() }
fn call(s: &Mutex<Store>, cmd: &str, args: Value) -> Value { dispatch(s,now(),cmd,args).unwrap() }
fn png() -> Vec<u8> {
    let image = image::DynamicImage::new_rgba8(16,16);
    let mut bytes=Cursor::new(Vec::new()); image.write_to(&mut bytes,image::ImageOutputFormat::Png).unwrap(); bytes.into_inner()
}
fn archive(files: &[(&str,Vec<u8>)]) -> String {
    let mut zip=zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name,bytes) in files { zip.start_file(*name,zip::write::FileOptions::default()).unwrap(); zip.write_all(bytes).unwrap(); }
    b64(&zip.finish().unwrap().into_inner())
}

#[test]
fn attachments_survive_crud_and_cross_device_sync() {
    let a=Mutex::new(Store::open_in_memory().unwrap());
    let attachment=call(&a,"attachments.prepare",json!({"name":"file.txt","mime":"text/plain","data":b64(b"hello")}));
    let saved=call(&a,"notes.save",json!({"note":{"title":"Files","body":"Test","attachments":[attachment.clone()]}}));
    let reminder=call(&a,"reminders.save",json!({"reminder":{"title":"Attached","start":"2026-10-30T13:00","attachments":[attachment.clone()]}}));
    assert_eq!(call(&a,"notes.list",json!({}))[0]["attachments"][0],attachment);
    assert_eq!(reminder["attachments"][0],attachment);
    let b=Store::open_in_memory().unwrap();
    let payload=a.lock().unwrap().make_payload(0,"data").unwrap();
    assert_eq!(b.apply_payload(&payload).unwrap().rejected,0);
    assert_eq!(b.list_notes(None,None).unwrap()[0].attachments.len(),1);
    assert_eq!(b.list_reminders(false).unwrap()[0].attachments[0].name,"file.txt");
    call(&a,"notes.delete",json!({"id":saved["id"]}));
    call(&a,"history.undo",json!({}));
    assert_eq!(call(&a,"notes.list",json!({}))[0]["attachments"][0],attachment);
}

#[test]
fn attachments_reject_tampering_paths_and_duplicates() {
    let s=Mutex::new(Store::open_in_memory().unwrap());
    assert!(dispatch(&s,now(),"attachments.prepare",json!({"name":"../bad","data":b64(b"x")})).is_err());
    let mut attachment=call(&s,"attachments.prepare",json!({"name":"x.txt","mime":"text/plain","data":b64(b"x")}));
    let duplicate=json!({"note":{"title":"Bad","attachments":[attachment.clone(),attachment.clone()]}});
    assert!(dispatch(&s,now(),"notes.save",duplicate).is_err());
    attachment["data"]=json!(b64(b"changed"));
    assert!(dispatch(&s,now(),"notes.save",json!({"note":{"title":"Bad","attachments":[attachment]}})).is_err());
    assert_eq!(call(&s,"notes.list",json!({})),json!([]));
}

#[test]
fn legacy_devices_cannot_silently_erase_new_attachments() {
    let s=Mutex::new(Store::open_in_memory().unwrap());
    let attachment=call(&s,"attachments.prepare",json!({"name":"keep.txt","mime":"text/plain","data":b64(b"keep me")}));
    call(&s,"notes.save",json!({"note":{"title":"Keep","attachments":[attachment]}}));
    let guard=s.lock().unwrap();
    let mut payload=guard.make_payload(0,"data").unwrap();
    assert_eq!(payload.v,2);
    payload.v=1;
    let note=payload.records.iter_mut().find(|r|r.kind=="note").unwrap();
    note.data.as_object_mut().unwrap().remove("attachments");
    note.data["title"]=json!("Old client edit");
    note.modified+=1000;
    assert!(guard.apply_payload(&payload).is_err());
    assert_eq!(guard.list_notes(None,None).unwrap()[0].attachments.len(),1);
    assert_eq!(guard.list_notes(None,None).unwrap()[0].title,"Keep");
}

#[test]
fn icon_packs_import_raster_only_and_reject_zip_slip() {
    let pack=features::import_icon_pack(&archive(&[("folder/settings.png",png()),("folder/plusS.png",png())])).unwrap();
    assert!(pack["settings"].as_str().unwrap().starts_with("data:image/png;base64,"));
    features::validate_icon(pack["settings"].as_str().unwrap()).unwrap();
    assert!(features::import_icon_pack(&archive(&[("../settings.png",png())])).is_err());
    assert!(features::import_icon_pack(&archive(&[("settings.svg",b"<svg onload='alert(1)'/>".to_vec())])).is_err());
    assert!(features::import_icon_pack(&archive(&[("a/settings.png",png()),("b/settings.png",png())])).is_err());
    assert!(features::import_icon_pack("broken").is_err());
}

#[test]
fn uploaded_icons_resize_and_sync_as_part_of_records() {
    let s=Mutex::new(Store::open_in_memory().unwrap());
    let image=call(&s,"icons.image",json!({"data":b64(&png())}));
    let r=call(&s,"reminders.save",json!({"reminder":{"title":"Icon","icon":image["icon"],"start":"2026-10-30T13:00"}}));
    assert_eq!(r["icon"],image["icon"]);
    assert!(features::optimize_icon(b"<svg/>").is_err());
    let n=call(&s,"notes.save",json!({"note":{"title":"Vector note","icon":"builtin:study"}}));
    assert_eq!(call(&s,"notes.list",json!({}))[0]["icon"],n["icon"]);
    let b=Store::open_in_memory().unwrap();
    b.apply_payload(&s.lock().unwrap().make_payload(0,"data").unwrap()).unwrap();
    assert_eq!(b.list_notes(None,None).unwrap()[0].icon,"builtin:study");
}

#[test]
fn undo_create_edit_delete_is_persistent_and_does_not_overwrite_external_changes() {
    let s=Mutex::new(Store::open_in_memory().unwrap());
    let n=call(&s,"notes.save",json!({"note":{"title":"Original"}}));
    assert_eq!(call(&s,"history.status",json!({}))["available"],true);
    call(&s,"history.undo",json!({}));
    assert_eq!(call(&s,"notes.list",json!({})),json!([]));
    let n=call(&s,"notes.save",json!({"note":{"title":"Original","id":0,"body":n["body"]}}));
    let mut edited=n.clone(); edited["title"]=json!("Edited");
    call(&s,"notes.save",json!({"note":edited}));
    call(&s,"history.undo",json!({}));
    assert_eq!(call(&s,"notes.list",json!({}))[0]["title"],"Original");
    call(&s,"notes.delete",json!({"id":n["id"]}));
    s.lock().unwrap().save_note(&Note {title:"External".into(),..Note::default()},now()).unwrap();
    assert_eq!(call(&s,"history.status",json!({}))["available"],false);
    assert!(dispatch(&s,now(),"history.undo",json!({})).is_err());
}

#[test]
fn failed_mutation_keeps_previous_undo() {
    let s=Mutex::new(Store::open_in_memory().unwrap());
    call(&s,"notes.save",json!({"note":{"title":"Undo me"}}));
    assert!(dispatch(&s,now(),"notes.save",json!({"note":{"title":"","body":""}})).is_err());
    call(&s,"history.undo",json!({}));
    assert_eq!(call(&s,"notes.list",json!({})),json!([]));
}

#[test]
fn password_guards_api_and_settings_cannot_bypass_lock() {
    let s=Mutex::new(Store::open_in_memory().unwrap());
    call(&s,"security.password",json!({"current":"","password":"correct horse"}));
    call(&s,"security.lock",json!({}));
    assert_eq!(call(&s,"security.status",json!({}))["locked"],true);
    for cmd in ["notes.list","sync.export","dropbox.status","icons.get","history.undo","settings.get"] {
        assert!(dispatch(&s,now(),cmd,json!({})).unwrap_err().to_string().contains("LOCKED"));
    }
    assert!(dispatch(&s,now(),"security.unlock",json!({"password":"wrong"})).is_err());
    call(&s,"security.unlock",json!({"password":"correct horse"}));
    let settings=call(&s,"settings.get",json!({}));
    assert!(settings.as_object().unwrap().keys().all(|k|!k.starts_with("private.")));
    assert!(dispatch(&s,now(),"settings.set",json!({"key":"private.password","value":""})).is_err());
    call(&s,"security.password",json!({"current":"correct horse","password":""}));
    assert_eq!(call(&s,"security.status",json!({}))["enabled"],false);
}

#[test]
fn dropbox_import_is_non_executing_and_secrets_are_not_exported() {
    let config="import os\nDROPBOX_APP_KEY = 'test-app-key'\nDROPBOX_REFRESH_TOKEN = 'super-private-refresh-token'\nENCRYPTION_KEYWORD = 'not-to-import'\n";
    let c=parse_credentials(config).unwrap();
    assert_eq!(c["app_key"],"test-app-key");
    assert!(c.get("ENCRYPTION_KEYWORD").is_none());
    assert!(parse_credentials("DROPBOX_APP_KEY = dangerous()\nDROPBOX_REFRESH_TOKEN='x'").is_err());
    assert!(parse_credentials("DROPBOX_APP_KEY='x'; os.remove('anything')\nDROPBOX_REFRESH_TOKEN='y'").is_err());
    let s=Mutex::new(Store::open_in_memory().unwrap());
    call(&s,"dropbox.configure",json!({"text":config}));
    assert_eq!(call(&s,"dropbox.status",json!({}))["connected"],true);
    for old in ["dropbox.run","dropbox.backup","dropbox.backups","dropbox.restore","dropbox.config"] {
        assert!(dispatch(&s,now(),old,json!({})).is_err(),"{old} must not export personal data");
    }
    for cmd in ["settings.get","sync.export","backup.export","dropbox.status"] {
        let text=call(&s,cmd,json!({})).to_string();
        assert!(!text.contains("super-private-refresh-token"),"{cmd}");
        assert!(!text.contains("not-to-import"),"{cmd}");
    }
    call(&s,"dropbox.disconnect",json!({}));
    assert_eq!(call(&s,"dropbox.status",json!({}))["connected"],false);
}

#[test]
fn backup_restores_attachments_settings_and_icons_but_not_credentials() {
    let a=Mutex::new(Store::open_in_memory().unwrap());
    call(&a,"sync.create",json!({}));
    let code=call(&a,"sync.status",json!({}))["code"].as_str().unwrap().to_string();
    call(&a,"notes.save",json!({"note":{"title":"Saved"}}));
    call(&a,"settings.set",json!({"key":"font_family","value":"serif"}));
    call(&a,"icons.import",json!({"data":archive(&[("settings.png",png())])}));
    let text=call(&a,"backup.export",json!({}))["text"].as_str().unwrap().to_string();
    assert!(text.starts_with("DPBACKUP1:")); assert!(!text.contains("Saved"));
    let b=Mutex::new(Store::open_in_memory().unwrap());
    assert!(dispatch(&b,now(),"backup.restore",json!({"text":text})).is_err());
    call(&b,"sync.join",json!({"code":code}));
    call(&b,"backup.restore",json!({"text":text}));
    assert_eq!(call(&b,"notes.list",json!({}))[0]["title"],"Saved");
    assert_eq!(call(&b,"settings.get",json!({}))["font_family"],"serif");
    assert!(call(&b,"icons.get",json!({}))["settings"].as_str().unwrap().starts_with("data:"));
    assert_eq!(call(&b,"dropbox.status",json!({}))["connected"],false);
}

#[test]
fn postponement_is_atomic_and_undoable() {
    let s=Mutex::new(Store::open_in_memory().unwrap());
    let r=call(&s,"reminders.save",json!({"reminder":{"title":"Move","start":"2026-10-30T12:00"}}));
    call(&s,"postpone",json!({"reminder_id":r["id"],"at":"2026-10-30T12:00","until":"2026-10-31T12:00"}));
    assert_eq!(call(&s,"reminders.list",json!({})).as_array().unwrap().len(),1);
    call(&s,"history.undo",json!({}));
    let rows=call(&s,"reminders.list",json!({"archived":true}));
    assert_eq!(rows.as_array().unwrap().len(),1);
    assert_eq!(rows[0]["start"],"2026-10-30T12:00:00");
    assert_eq!(rows[0]["archived"],false);
}

#[test]
fn invalid_postpone_rolls_back_new_reminder() {
    let s=Mutex::new(Store::open_in_memory().unwrap());
    let r=call(&s,"reminders.save",json!({"reminder":{"title":"Move","start":"2026-10-30T12:00"}}));
    assert!(dispatch(&s,now(),"postpone",json!({"reminder_id":r["id"],"at":"2026-10-30T12:01","until":"2026-10-31T12:00"})).is_err());
    assert_eq!(call(&s,"reminders.list",json!({"archived":true})).as_array().unwrap().len(),1);
}

#[test]
fn local_encrypted_credentials_survive_restart_and_start_locked() {
    let dir=std::env::temp_dir().join(format!("planner-private-test-{}",sync_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path=dir.join("diary.sqlite3");
    {
        let s=Mutex::new(Store::open(&path).unwrap());
        call(&s,"dropbox.configure",json!({"app_key":"a","refresh_token":"do-not-store-plaintext","access_token":"","app_secret":""}));
        call(&s,"security.password",json!({"password":"test password"}));
    }
    let bytes=std::fs::read(&path).unwrap();
    assert!(!bytes.windows(b"do-not-store-plaintext".len()).any(|s|s==b"do-not-store-plaintext"));
    {
        let s=Mutex::new(Store::open(&path).unwrap());
        assert_eq!(call(&s,"security.status",json!({}))["locked"],true);
        call(&s,"security.unlock",json!({"password":"test password"}));
        assert_eq!(call(&s,"dropbox.status",json!({}))["connected"],true);
    }
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(dir.join(".dayplanner-private-key")).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
fn sync_id() -> String { derive_keys(&planner_core::sync::generate_code()).topic }
