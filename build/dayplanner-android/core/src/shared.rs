//! Shared categories are intentionally outside the personal sync/achievement tables.
use std::collections::BTreeMap;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::{Store, Result, features::{invalid, random}, model::Attachment, sync};

pub(crate) fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS shared_categories(
        id TEXT PRIMARY KEY, remote TEXT UNIQUE, name TEXT NOT NULL,
        role TEXT NOT NULL, document TEXT NOT NULL, last_ok INTEGER, error TEXT);")?;
    Ok(())
}
pub fn new_id() -> Result<String> { Ok(sync::hex(&random::<16>()?)) }
fn valid_id(s: &str) -> bool { s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()) }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub icon: String,
    pub done: bool,
    pub deleted: bool,
    pub modified: i64,
    pub author: String,
    pub attachments: Vec<Attachment>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub format: String,
    pub category: String,
    pub name: String,
    pub entries: BTreeMap<String, Entry>,
}
impl Document {
    pub fn new(name: &str) -> Result<Self> {
        let doc = Self {format:"dayplanner-shared-category-v1".into(),category:new_id()?,name:name.trim().into(),entries:BTreeMap::new()};
        doc.validate()?; Ok(doc)
    }
    pub fn validate(&self) -> Result<()> {
        if self.format != "dayplanner-shared-category-v1" || !valid_id(&self.category) || self.name.trim().is_empty() || self.name.len()>240 || self.entries.len()>10000 { return Err(invalid("Некорректная общая категория")); }
        for (id, e) in &self.entries {
            if id != &e.id || !valid_id(id) || !matches!(e.kind.as_str(),"task"|"note") || e.title.len()>1000 || e.body.len()>100_000 || e.author.len()>128 || e.modified<0 || e.modified>chrono::Utc::now().timestamp_millis()+86400000 { return Err(invalid("Повреждённая общая запись")); }
            crate::features::validate_icon(&e.icon)?;
            crate::features::validate_attachments(&e.attachments)?;
        }
        Ok(())
    }
    /// Whole-entry last-write-wins; deterministic tie break, including tombstones.
    pub fn merge(&mut self, other: &Self) -> Result<()> {
        other.validate()?;
        if self.category != other.category { return Err(invalid("Это другая общая категория; данные не смешаны")); }
        for (id, e) in &other.entries {
            let replace = match self.entries.get(id) {
                None => true,
                Some(old) => (e.modified,serde_json::to_string(e)?) > (old.modified,serde_json::to_string(old)?),
            };
            if replace { self.entries.insert(id.clone(),e.clone()); }
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct Category { pub id:String, pub remote:Option<String>, pub role:String, pub document:Document }
impl Store {
    pub fn shared_get(&self, id:&str) -> Result<Category> {
        let row:(Option<String>,String,String) = self.conn.query_row("SELECT remote,role,document FROM shared_categories WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        Ok(Category{id:id.into(),remote:row.0,role:row.1,document:serde_json::from_str(&row.2)?})
    }
    pub fn shared_add(&self, remote:&str, role:&str, doc:&Document) -> Result<String> {
        doc.validate()?;
        let existing:Option<String> = self.conn.query_row("SELECT id FROM shared_categories WHERE remote=?1",[remote],|r|r.get(0)).optional()?;
        if let Some(id)=existing { return Ok(id); }
        let id=new_id()?;
        self.conn.execute("INSERT INTO shared_categories(id,remote,name,role,document) VALUES(?1,?2,?3,?4,?5)",params![id,remote,doc.name,role,serde_json::to_string(doc)?])?;
        Ok(id)
    }
    pub fn shared_list(&self) -> Result<Value> {
        let mut st=self.conn.prepare("SELECT id,name,role,remote,last_ok,error FROM shared_categories ORDER BY name")?;
        let rows=st.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"role":r.get::<_,String>(2)?,"remote":r.get::<_,Option<String>>(3)?,"last_ok":r.get::<_,Option<i64>>(4)?,"error":r.get::<_,Option<String>>(5)?})))?;
        Ok(Value::Array(rows.collect::<std::result::Result<Vec<_>,_>>()?))
    }
    pub fn shared_write(&self, id:&str, doc:&Document) -> Result<()> {
        doc.validate()?;
        self.conn.execute("UPDATE shared_categories SET document=?2,name=?3 WHERE id=?1",params![id,serde_json::to_string(doc)?,doc.name])?;
        Ok(())
    }
    pub fn shared_dispatch(&self, cmd:&str, args:&Value) -> Result<Value> {
        if cmd=="list" { return self.shared_list(); }
        let id=args["id"].as_str().unwrap_or("");
        let mut cat=self.shared_get(id)?;
        match cmd {
            "get" => Ok(json!({"id":id,"role":cat.role,"remote":cat.remote,"document":cat.document})),
            "detach" => {
                self.conn.execute("UPDATE shared_categories SET remote=NULL,role='local',error=NULL WHERE id=?1",[id])?;
                Ok(Value::Null)
            },
            "save" => {
                if !matches!(cat.role.as_str(),"owner"|"editor"|"local") { return Err(invalid("Категория доступна только для чтения")); }
                let mut e:Entry=serde_json::from_value(args["entry"].clone())?;
                if e.id.is_empty() { e.id=new_id()?; }
                let current=cat.document.entries.get(&e.id);
                let expected=args["expected"].as_i64().unwrap_or(0);
                if current.map(|e|e.modified).unwrap_or(0)!=expected { return Err(invalid("Запись уже изменена. Откройте её заново, чтобы не затереть правки")); }
                e.modified=chrono::Utc::now().timestamp_millis().max(cat.document.entries.values().map(|e|e.modified).max().unwrap_or(0)+1);
                e.author=self.sync_config()?.device;
                cat.document.entries.insert(e.id.clone(),e.clone());
                self.shared_write(id,&cat.document)?;
                Ok(json!(e))
            },
            _ => Err(invalid("Неизвестная команда общей категории")),
        }
    }
}
use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolated_from_personal_export_and_scores_and_detach_keeps_copy() {
        let s=Store::open_in_memory().unwrap();
        let doc=Document::new("Работа").unwrap();
        let id=s.shared_add("123","editor",&doc).unwrap();
        let e=json!({"id":"","kind":"task","title":"Shared only","body":"","icon":"builtin:task","done":true,"deleted":false,"modified":0,"author":"","attachments":[]});
        s.shared_dispatch("save",&json!({"id":id,"entry":e})).unwrap();
        assert!(!s.sync_export_text().unwrap().contains("Shared only"));
        assert!(s.list_reminders(true).unwrap().is_empty());
        assert!(s.all_completions().unwrap().is_empty());
        assert!(s.list_categories().unwrap().iter().all(|c|c.name!="Shared only"));
        s.shared_dispatch("detach",&json!({"id":id})).unwrap();
        assert_eq!(s.shared_get(&id).unwrap().document.entries.len(),1);
        assert!(s.shared_get(&id).unwrap().remote.is_none());
    }
    #[test]
    fn merging_is_scoped_deterministic_and_deletions_persist() {
        let mut a=Document::new("Общие").unwrap();
        let mut b=a.clone();
        let id=new_id().unwrap();
        let e=Entry{id:id.clone(),kind:"note".into(),title:"Hello".into(),body:"".into(),icon:"builtin:note".into(),done:false,deleted:false,modified:1,author:"A".into(),attachments:vec![]};
        a.entries.insert(id.clone(),e.clone());
        b.entries.insert(id.clone(),Entry{deleted:true,modified:2,..e});
        a.merge(&b).unwrap(); b.merge(&a).unwrap();
        assert!(a.entries[&id].deleted);
        assert_eq!(a.entries,b.entries);
        assert!(a.merge(&Document::new("Other").unwrap()).is_err());
    }
    #[test]
    fn readonly_and_stale_edits_rejected() {
        let s=Store::open_in_memory().unwrap();
        let id=s.shared_add("123","viewer",&Document::new("Read only").unwrap()).unwrap();
        assert!(s.shared_dispatch("save",&json!({"id":id,"entry":{}})).is_err());
        let id=s.shared_add("456","editor",&Document::new("Writable").unwrap()).unwrap();
        let entry=json!({"id":"","kind":"note","title":"First","body":"","icon":"builtin:note","done":false,"deleted":false,"modified":0,"author":"","attachments":[]});
        let saved=s.shared_dispatch("save",&json!({"id":id,"entry":entry})).unwrap();
        assert!(s.shared_dispatch("save",&json!({"id":id,"entry":saved,"expected":0})).is_err());
        assert_eq!(s.shared_get(&id).unwrap().document.entries.len(),1);
    }
}
