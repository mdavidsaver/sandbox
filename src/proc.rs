use std::collections::HashMap;
use std::{env, ffi, fmt};

use libc;
use signal_hook;
use signal_hook::iterator::Signals;

use log::{debug, warn};

use super::err::{Error, Result};

/// Managed (child) process
#[derive(Debug)]
pub struct Proc {
    pid: libc::pid_t,
    done: bool,
    code: i32,
}

impl Proc {
    pub fn manage(pid: libc::pid_t) -> Proc {
        assert!(pid > 0);
        Proc {
            pid: pid,
            done: false,
            code: -1, // poison
        }
    }

    pub fn id(&self) -> libc::pid_t {
        self.pid
    }

    /// Send signal to process
    pub fn signal(&self, sig: libc::c_int) -> Result<()> {
        if !self.done {
            debug!("signal PID {} with {}", self.pid, sig);
            unsafe {
                if 0 != libc::kill(self.pid, sig) {
                    return Err(Error::last_os_error(format!(
                        "Unable to signal {} with {}",
                        self.pid, sig
                    )));
                }
            }
        }
        Ok(())
    }

    /// Send SIGKILL to process
    pub fn kill(&self) -> Result<()> {
        self.signal(libc::SIGKILL)
    }

    /// Block current process until child exits.
    ///
    pub fn park(&mut self) -> Result<i32> {
        if self.done {
            return Ok(self.code);
        }

        let signals = Signals::new(&[
            signal_hook::SIGTERM,
            signal_hook::SIGINT,
            signal_hook::SIGQUIT,
            signal_hook::SIGCHLD,
        ])
        .map_err(|e| Error::os("Install signal handler", e))?;
        let mut isig = signals.forever();

        let mut cnt = 0;

        loop {
            match trywaitpid(self.pid) {
                Err(err) => return Err(err),
                Ok(TryWait::Busy) => (),
                Ok(TryWait::Done(_child, sts)) => {
                    debug!("park() -> {}", sts);
                    self.done = true;
                    self.code = sts;
                    return Ok(sts);
                }
            }
            debug!("Waiting for PID {}", self.pid);

            match isig.next() {
                Some(signal_hook::SIGCHLD) => {
                    debug!("SIGCHLD");
                    // loop around to test child
                }
                Some(sig) => {
                    debug!("SIG {}", sig);
                    // we are being interrupted.
                    // be delicate with child at first
                    let num = if cnt < 2 { sig } else { libc::SIGKILL };
                    cnt += 1;
                    self.signal(num)?;
                }
                None => {
                    unreachable!();
                }
            }
        }
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        if let Err(err) = self.kill() {
            warn!("unable to kill managed PID {} : {}", self.pid, err);
        }
    }
}

impl fmt::Display for Proc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.done {
            write!(f, "PID {} Exit with {}", self.pid, self.code)
        } else {
            write!(f, "PID {}", self.pid)
        }
    }
}

pub enum TryWait {
    Busy,
    Done(libc::pid_t, i32),
}

/// Wraps waitpid()
pub fn trywaitpid(pid: libc::pid_t) -> Result<TryWait> {
    let mut sts = 0;
    unsafe {
        let ret = libc::waitpid(pid, &mut sts, libc::WNOHANG);
        if ret == -1 {
            Err(Error::last_os_error(format!("waitpid({})", pid)))
        } else if ret == 0 {
            Ok(TryWait::Busy)
        } else {
            Ok(TryWait::Done(ret, libc::WEXITSTATUS(sts)))
        }
    }
}

pub struct Exec {
    cmd: ffi::CString,
    args: Vec<ffi::CString>,
    env: HashMap<String, ffi::CString>,
}

impl Exec {
    pub fn new<T: AsRef<str>>(cmd: T) -> Result<Exec> {
        let mut es = HashMap::new();

        // initially populate with process environment
        for (k, v) in env::vars() {
            es.insert(
                k.clone(),
                ffi::CString::new(format!("{}={}", &k, &v).as_bytes())?,
            );
        }

        Ok(Exec {
            cmd: ffi::CString::new(cmd.as_ref())?,
            args: vec![],
            env: es,
        })
    }

    pub fn args<I>(&mut self, args: I) -> Result<&mut Self>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        for s in args.into_iter() {
            self.args.push(ffi::CString::new(s.as_ref())?);
        }
        Ok(self)
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.env.clear();
        self
    }

    pub fn env<'a, T>(&mut self, name: T, value: T) -> Result<&mut Self>
    where
        T: Into<&'a str>,
    {
        self.env
            .insert(name.into().to_string(), ffi::CString::new(value.into())?);
        Ok(self)
    }

    pub fn env_remove<'a, T: Into<&'a str>>(&mut self, name: T) -> &mut Self {
        self.env.remove(name.into());
        self
    }

    pub fn exec(&self) -> Result<()> {
        let cmd = self.cmd.as_ptr();
        let mut args: Vec<*const libc::c_char> = self.args.iter().map(|s| s.as_ptr()).collect();
        let mut env: Vec<*const libc::c_char> = self.env.iter().map(|(_k, v)| v.as_ptr()).collect();
        // arrays must be null terminated
        args.push(::std::ptr::null());
        env.push(::std::ptr::null());

        Err(unsafe {
            libc::execvpe(cmd, args.as_ptr(), env.as_ptr());
            // only returns on error
            Error::last_os_error(format!(
                "exec cmd={:?} args={:?} env={:?}",
                self.cmd, self.args, self.env
            ))
        })
    }
}

pub enum Fork {
    Parent(Proc),
    Child,
}

pub fn fork() -> Result<Fork> {
    unsafe {
        match libc::fork() {
            err if err < 0 => return Err(Error::last_os_error("fork")),
            0 => Ok(Fork::Child),
            pid => Ok(Fork::Parent(Proc::manage(pid))),
        }
    }
}
