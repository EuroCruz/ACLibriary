use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Eof { at: usize, need: usize },
    Bad(&'static str),
    Msg(String),
}

pub type Res<T> = Result<T, Error>;

pub fn bad<T>(m: &'static str) -> Res<T> {
    Err(Error::Bad(m))
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Eof { at, need } => write!(f, "eof at {at}, need {need}"),
            Error::Bad(m) => f.write_str(m),
            Error::Msg(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Msg(e.to_string())
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(e: std::str::Utf8Error) -> Error {
        Error::Msg(e.to_string())
    }
}

impl From<String> for Error {
    fn from(m: String) -> Error {
        Error::Msg(m)
    }
}
