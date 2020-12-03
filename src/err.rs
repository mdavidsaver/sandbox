use std::path::{Path, PathBuf};
use std::{error, fmt, io};

#[derive(Debug)]
pub enum Error {
    File {
        op: String,
        name: PathBuf,
        io: io::Error,
    },
    OS {
        op: String,
        io: io::Error,
    },
    TooLong,
    NotIPv4,
    BadStr,
    UIDMap,
    ParseError {
        msg: String,
        name: PathBuf,
    },
    MissingMount,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Annotate I/O error
    pub fn file<S: AsRef<str>, P: AsRef<Path>>(desc: S, path: P, err: io::Error) -> Self {
        Error::File {
            op: desc.as_ref().to_string(),
            name: path.as_ref().to_path_buf(),
            io: err,
        }
    }

    /// Annotate Error::last_os_error()
    pub fn last_file_error<S: AsRef<str>, P: AsRef<Path>>(desc: S, path: P) -> Self {
        Self::file(desc, path, io::Error::last_os_error())
    }

    pub fn os<S: AsRef<str>>(desc: S, err: io::Error) -> Self {
        Self::OS {
            op: desc.as_ref().to_string(),
            io: err,
        }
    }

    /// Annotate Error::last_os_error()
    pub fn last_os_error<S: AsRef<str>>(desc: S) -> Self {
        Self::os(desc, io::Error::last_os_error())
    }

    pub fn parse<M: AsRef<str>, P: AsRef<Path>>(msg: M, path: P) -> Self {
        Self::ParseError {
            msg: msg.as_ref().to_string(),
            name: path.as_ref().to_path_buf(),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::File { io, .. } => Some(io),
            Self::OS { io, .. } => Some(io),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File { op, name, io } => {
                write!(f, "File {} with {} : {}", op, name.display(), io)
            }
            Self::OS { op, io } => write!(f, "OS {} : {}", op, io),
            Self::TooLong => write!(f, "Interface name too long"),
            Self::NotIPv4 => write!(f, "Interface address not IPv4"),
            Self::BadStr => write!(f, "String can not contain nil"),
            Self::UIDMap => write!(f, "newuidmap"),
            Self::ParseError { msg, name } => write!(f, "Error: {} while parsing {}", msg, name.display()),
            Self::MissingMount => write!(f, "Missing mount point info"),
        }
    }
}

impl From<std::ffi::NulError> for Error {
    fn from(_inp: std::ffi::NulError) -> Self {
        Error::BadStr
    }
}

impl From<std::num::ParseIntError> for Error {
    fn from(_inp: std::num::ParseIntError) -> Self {
        Error::BadStr
    }
}
