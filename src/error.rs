use std::fmt;

#[derive(Debug)]
pub enum Error {
    Ghc(String),
    Io(std::io::Error),
    Session(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Ghc(msg) => write!(f, "ghci: {msg}"),
            Error::Io(e) => write!(f, "io: {e}"),
            Error::Session(msg) => write!(f, "session: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
