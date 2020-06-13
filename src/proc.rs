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
        where T: Into<Vec<u8>>
    {
        let mut es = HashMap::new();
        env::vars().try_for_each(|(k,v)| {
            es.insert(k.clone(), ffi::CString::new(format!("{}={}", &k,&v).as_bytes())?);
            Ok(())
        })?;

        Ok(Exec {
            cmd: ffi::CString::new(cmd)?,
            args: vec![],
            env: es,
        })
    }

    pub fn args<I>(&mut self, args: I) -> Result<&mut Self, ffi::NulError>
        where I: IntoIterator,
              I::Item: Into<Vec<u8>>
    {
        args.into_iter().try_for_each(|s| {
            self.args.push(ffi::CString::new(s)?);
            Ok(())
        })?;
        Ok(self)
    }

    pub fn exec(&self) -> Result<(), AnnotatedError> {
        let cmd = self.cmd.as_ptr();
        let mut args: Vec<*const libc::c_char> = self.args.iter().map(|s| { s.as_ptr() }).collect();
        let mut env: Vec<*const libc::c_char> = self.env.iter().map(|(_k,v)| { v.as_ptr() }).collect();
        // arrays must be null terminated
        args.push(::std::ptr::null());
        env.push(::std::ptr::null());

        unsafe {
            libc::execvpe(cmd, args.as_ptr(), env.as_ptr());
            // only returns on error
            Err(io::Error::last_os_error()
            .annotate(""))
        }
    }
}
