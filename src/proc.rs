use std::{io, env, ffi};
use std::collections::HashMap;

use libc;

use super::{Annotatable, AnnotatedError};

pub struct Exec {
    cmd: ffi::CString,
    args: Vec<ffi::CString>,
    env: HashMap<String, ffi::CString>,
}

impl Exec {
    pub fn new<T>(cmd: T) -> Result<Exec, ffi::NulError>
        where T: AsRef<str>
    {
        let mut es = HashMap::new();

        // initially populate with process environment
        env::vars().try_for_each(|(k,v)| {
            es.insert(k.clone(), ffi::CString::new(format!("{}={}", &k,&v).as_bytes())?);
            Ok(())
        })?;

        Ok(Exec {
            cmd: ffi::CString::new(cmd.as_ref())?,
            args: vec![],
            env: es,
        })
    }

    pub fn args<I>(&mut self, args: I) -> Result<&mut Self, ffi::NulError>
        where I: IntoIterator,
              I::Item: AsRef<str>
    {
        args.into_iter().try_for_each(|s| {
            self.args.push(ffi::CString::new(s.as_ref())?);
            Ok(())
        })?;
        Ok(self)
    }

    pub fn env_clear(&mut self) -> &mut Self {
        self.env.clear();
        self
    }

    pub fn env<'a, T>(&mut self, name: T, value: T) -> Result<&mut Self, ffi::NulError>
        where T: Into<&'a str>
    {
        self.env.insert(name.into().to_string(),
                        ffi::CString::new(value.into())?);
        Ok(self)
    }

    pub fn env_remove<'a, T: Into<&'a str>>(&mut self, name: T) -> &mut Self {
        self.env.remove(name.into());
        self
    }

    pub fn exec(&self) -> Result<(), AnnotatedError> {
        let cmd = self.cmd.as_ptr();
        let mut args: Vec<*const libc::c_char> = self.args.iter().map(|s| { s.as_ptr() }).collect();
        let mut env: Vec<*const libc::c_char> = self.env.iter().map(|(_k,v)| { v.as_ptr() }).collect();
        // arrays must be null terminated
        args.push(::std::ptr::null());
        env.push(::std::ptr::null());

        Err(unsafe {
            libc::execvpe(cmd, args.as_ptr(), env.as_ptr());
            // only returns on error
            io::Error::last_os_error()
        }.annotate(&format!("exec cmd={:?} args={:?} env={:?}",
                            self.cmd, self.args, self.env)))
    }
}
