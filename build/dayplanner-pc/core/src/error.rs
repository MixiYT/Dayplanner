use std::fmt;

#[derive(Debug)]
pub enum Error {
    Db(rusqlite::Error),
    Json(serde_json::Error),
    /// Некорректные данные от пользователя.
    Invalid(String),
    NotFound(String),
    /// Ошибка связи с сервером рейтинга.
    Net(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Db(e) => write!(f, "ошибка базы данных: {e}"),
            Error::Json(e) => write!(f, "ошибка данных: {e}"),
            Error::Invalid(m) => write!(f, "{m}"),
            Error::NotFound(m) => write!(f, "не найдено: {m}"),
            Error::Net(m) => write!(f, "сеть: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Db(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}
