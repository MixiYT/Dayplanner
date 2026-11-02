//! Единый формат дат и времени. Интерфейс присылает "2026-10-27T09:00"
//! (без секунд), ядро всегда отдаёт "2026-10-27T09:00:00".

pub mod dt {
    use chrono::NaiveDateTime;
    use serde::{Deserialize, Deserializer, Serializer};

    pub const OUT: &str = "%Y-%m-%dT%H:%M:%S";

    pub fn format(v: &NaiveDateTime) -> String {
        v.format(OUT).to_string()
    }

    pub fn parse(s: &str) -> Result<NaiveDateTime, String> {
        for f in [
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%dT%H:%M:%S%.f",
            "%Y-%m-%dT%H:%M",
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%d %H:%M",
        ] {
            if let Ok(v) = NaiveDateTime::parse_from_str(s, f) {
                return Ok(v);
            }
        }
        Err(format!("неверная дата или время: {s}"))
    }

    pub fn serialize<S: Serializer>(v: &NaiveDateTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveDateTime, D::Error> {
        let s = String::deserialize(d)?;
        parse(&s).map_err(serde::de::Error::custom)
    }
}

pub mod tm {
    use chrono::NaiveTime;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn format(v: &NaiveTime) -> String {
        v.format("%H:%M").to_string()
    }

    pub fn parse(s: &str) -> Result<NaiveTime, String> {
        for f in ["%H:%M", "%H:%M:%S"] {
            if let Ok(v) = NaiveTime::parse_from_str(s, f) {
                return Ok(v);
            }
        }
        Err(format!("неверное время: {s}"))
    }

    pub fn serialize<S: Serializer>(v: &NaiveTime, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<NaiveTime, D::Error> {
        let s = String::deserialize(d)?;
        parse(&s).map_err(serde::de::Error::custom)
    }
}
