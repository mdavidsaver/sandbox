use std::path::{Path, PathBuf};
use libc;

use log::{error};

use super::err::{Error, Result};
use super::path;

#[derive(Debug)]
pub struct TempDir {
    name: PathBuf,
}

impl TempDir {
    pub fn new() -> Result<TempDir> {
        let template = path!(std::env::temp_dir(), "sandbox-XXXXXX");
        let template = std::ffi::CString::new(template.to_str().unwrap())?;
        unsafe {
            let temp = template.as_ptr();
            let ret = libc::mkdtemp(temp as *mut libc::c_char); // modifies template
            if ret.is_null() {
                return Err(Error::last_os_error("mkdtemp"));
            }
        }
        Ok(TempDir {
            name: PathBuf::from(template.into_string()?),
        })
    }

    pub fn path(&self) -> &Path {
        &self.name
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_dir_all(&self.name) {
            error!("Unable to remove temporary directory: {} : {}", self.name.display(), err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tempdir() {
        let tdir = TempDir::new().unwrap();
        assert!(std::fs::metadata(tdir.path()).unwrap().is_dir(), "{:?}", tdir);
    }
}
