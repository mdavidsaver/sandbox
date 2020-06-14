use std::ffi::CString;
use std::io::Error;
use std::net::TcpStream;
use std::path::Path;
use std::{error, fmt, fs};

use std::os::unix::io::FromRawFd;

use libc;

use log::debug;

mod ext;

mod capability;
pub use capability::*;

mod proc;
pub use proc::*;

mod user;
pub use user::*;

pub fn socketpair() -> Result<(TcpStream, TcpStream), AnnotatedError> {
    let mut fds = vec![0, 2];
    unsafe {
        if 0 != libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) {
            return Err(Error::last_os_error().annotate("socketpair"));
        }
        Ok((
            TcpStream::from_raw_fd(fds[0]),
            TcpStream::from_raw_fd(fds[1]),
        ))
    }
}

pub fn unshare(flags: libc::c_int) -> Result<(), Error> {
    debug!("unshare(0x{:x})", flags);
    unsafe {
        if libc::unshare(flags) != 0 {
            return Err(Error::last_os_error());
        }
    }
    Ok(())
}

pub fn mount<A, B, C>(
    src: A,
    target: B,
    fstype: C,
    flags: libc::c_ulong,
) -> Result<(), Box<dyn error::Error + 'static>>
where
    A: AsRef<Path>,
    B: AsRef<Path>,
    C: AsRef<Path>,
{
    mount_with_data(src, target, fstype, flags, "")
}

pub fn mount_with_data<A, B, C, D>(
    src: A,
    target: B,
    fstype: C,
    flags: libc::c_ulong,
    data: D,
) -> Result<(), Box<dyn error::Error + 'static>>
where
    A: AsRef<Path>,
    B: AsRef<Path>,
    C: AsRef<Path>,
    D: AsRef<Path>,
{
    let csrc = CString::new(src.as_ref().to_string_lossy().as_ref())?;
    let ctarget = CString::new(target.as_ref().to_string_lossy().as_ref())?;
    let cfstype = CString::new(fstype.as_ref().to_string_lossy().as_ref())?;
    let cdata = CString::new(data.as_ref().to_string_lossy().as_ref())?;
    debug!(
        "mount({:?},{:?},{:?},0x{:x},{:?})",
        csrc, ctarget, cfstype, flags, cdata
    );
    unsafe {
        if 0 != libc::mount(
            csrc.as_ptr() as *const i8,
            ctarget.as_ptr() as *const i8,
            cfstype.as_ptr() as *const i8,
            flags,
            cdata.as_ptr() as *const libc::c_void,
        ) {
            Err(Error::last_os_error().annotate(&format!(
                "mount src={:?} target={:?} fs={:?} flags=0x{:x} data=",
                src.as_ref(),
                target.as_ref(),
                fstype.as_ref(),
                flags
            )))?;
        }
    }
    Ok(())
}

pub fn create_file<P: AsRef<Path>>(fname: P, perm: libc::mode_t) -> Result<fs::File, Error> {
    let rawname = CString::new(fname.as_ref().to_string_lossy().as_ref())?;
    let fd;
    unsafe {
        fd = libc::creat(rawname.as_ptr(), perm);
        if fd < 0 {
            Err(Error::last_os_error())
        } else {
            Ok(fs::File::from_raw_fd(fd))
        }
    }
}

#[derive(Debug)]
pub struct AnnotatedError {
    msg: String,
    err: Option<Box<dyn error::Error + 'static>>,
}

impl AnnotatedError {
    pub fn new<S: AsRef<str>>(msg: S) -> AnnotatedError {
        AnnotatedError {
            msg: msg.as_ref().to_string(),
            err: None,
        }
    }
}

impl error::Error for AnnotatedError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.err.as_ref().map(|e| e.as_ref())
    }
}

impl fmt::Display for AnnotatedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.msg)
    }
}

/// Add a context message to an Error
pub trait Annotatable: error::Error {
    /// Turn any Error into an AnnotatedError
    fn annotate<S: AsRef<str>>(self, msg: S) -> AnnotatedError
    where
        Self: Sized + 'static,
    {
        AnnotatedError {
            msg: msg.as_ref().to_string(),
            err: Some(Box::new(self)),
        }
    }
}

// allow any Error to be annotated
impl<E: error::Error + ?Sized> Annotatable for E {}

pub trait AnnotateResult {
    type Value;
    fn annotate<S: AsRef<str>>(self, msg: S) -> Result<Self::Value, AnnotatedError>;
}

// Allow Result to be annotated
impl<V, E: Annotatable + 'static> AnnotateResult for Result<V, E> {
    type Value = V;
    fn annotate<S: AsRef<str>>(self, msg: S) -> Result<Self::Value, AnnotatedError> {
        self.map_err(|e| e.annotate(msg.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn test_pair() {
        let (mut a, mut b) = socketpair().expect("socketpair");
        a.set_nonblocking(true).unwrap();
        b.set_nonblocking(true).unwrap();

        a.write_all("msg".as_bytes()).unwrap();
        let mut buf = vec![0; 4];
        let n = b.read(&mut buf).unwrap();
        assert_eq!(n, 3);
        assert_eq!(&buf[0..3], "msg".as_bytes());
    }
}
