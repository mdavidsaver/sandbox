use std::{error, fmt};
use std::io::Error;
use std::net::TcpStream;
use std::ffi::CString;

use std::os::unix::io::FromRawFd;

use libc;
use signal_hook;
use signal_hook::iterator::Signals;

mod ext;

mod capability;
pub use capability::*;

mod proc;
pub use proc::*;

pub fn socketpair() -> Result<(TcpStream, TcpStream), AnnotatedError> {
    let mut fds = vec![0, 2];
    unsafe {
        if 0!=libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) {
            return Err(Error::last_os_error().annotate("socketpair"));
        }
        Ok((
            TcpStream::from_raw_fd(fds[0]),
            TcpStream::from_raw_fd(fds[1]),
        ))
    }
}

pub enum TryWait {
    Nope,
    Done(libc::pid_t, i32),
}

pub fn trywaitpid(pid: libc::pid_t) -> Result<TryWait, Error> {
   let mut sts = 0;
    unsafe {
        let ret = libc::waitpid(pid, &mut sts, libc::WNOHANG);
        if ret==-1 {
            Err(Error::last_os_error())
        } else if ret==0 {
            Ok(TryWait::Nope)
        } else {
            Ok(TryWait::Done(ret, libc::WEXITSTATUS(sts)))
        }
    }
}

/// Wait for child PID to exit.  Kill child if we are signaled
pub fn park(pid: libc::pid_t) -> Result<i32, Error> {
    let signals = Signals::new(&[
        signal_hook::SIGTERM,
        signal_hook::SIGINT,
        signal_hook::SIGQUIT,
        signal_hook::SIGCHLD,
    ])?;
    let mut cnt = 0;
    for sig in signals.forever() {
        match sig as libc::c_int {
        signal_hook::SIGCHLD => {
            // child has (probably) exited
            match trywaitpid(pid) {
            Err(err) => return Err(err),
            Ok(TryWait::Nope) => (),
            Ok(TryWait::Done(_child, sts)) => return Ok(sts),
            }
        },
        sig => {
            // we are being interrupted.
            // be delicate with child at first
            let num = if cnt<3 { sig } else { libc::SIGKILL };
            cnt+=1;
            kill(pid, num)?;
        },
        }
    }
    unreachable!();
}

pub fn unshare(flags: libc::c_int) -> Result<(), Error> {
    unsafe {
        if libc::unshare(flags) !=0 {
            return Err(Error::last_os_error());
        }
    }
    Ok(())
}

pub fn mount<S>(src: S,
             target: S,
             fstype: S,
             flags: libc::c_ulong,
             data: Option<S>
             ) -> Result<(), AnnotatedError>
    where S: AsRef<str>
{
    let csrc = CString::new(src.as_ref()).expect("nil src");
    let ctarget = CString::new(target.as_ref()).expect("nil target");
    let cfstype = CString::new(fstype.as_ref()).expect("nil fstype");
    let cdata = data.map(|s| {
        CString::new(s.as_ref()).expect("nil data")
    });
    unsafe {
        let pdata = match cdata {
        Some(d) => d.as_ptr() as *const libc::c_void,
        None => ::std::ptr::null(),
        };
        if 0!=libc::mount(csrc.as_ptr() as *const i8,
                          ctarget.as_ptr() as *const i8,
                          cfstype.as_ptr() as *const i8,
                          flags,
                          pdata)
        {
            return Err(Error::last_os_error()
                .annotate(&format!("mount src={:?} target={:?} fs={:?} flags=0x{:x} data=",
                                  src.as_ref(), target.as_ref(), fstype.as_ref(), flags)));
        }
    }
    Ok(())
}

pub fn kill(pid: libc::pid_t, sig: libc::c_int) -> Result<(), Error> {
    unsafe {
        if 0!=libc::kill(pid, sig) {
            return Err(Error::last_os_error());
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct AnnotatedError {
    msg: String,
    err: Option<Box<dyn error::Error + 'static>>,
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
    fn annotate(self, msg:&str) -> AnnotatedError
        where Self: Sized + 'static
    {
        AnnotatedError {
            msg: msg.to_string(),
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
impl<V,E: Annotatable + 'static> AnnotateResult for Result<V,E> {
    type Value = V;
    fn annotate<S: AsRef<str>>(self, msg: S) -> Result<Self::Value, AnnotatedError> {
        self.map_err(|e| { e.annotate(msg.as_ref()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Write, Read};

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
