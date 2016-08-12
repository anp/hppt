use std::convert::From;
use std::io;

pub type HpptResult<T> = Result<T, HpptError>;

// TODO make parsing error more nuanced for reporting to client
#[derive(Debug)]
pub enum HpptError {
    Parsing,
    IncompleteRequest,
    UnsupportedHttpVersion,
    IoError(io::Error),
}

impl From<io::Error> for HpptError {
    fn from(e: io::Error) -> Self {
        HpptError::IoError(e)
    }
}
