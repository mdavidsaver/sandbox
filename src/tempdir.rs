//! Manage a temporary directory

use libc;
use std::path::{Path, PathBuf};

use log::{debug, error};

use super::err::{Error, Result};
use super::path;

/// A temporary directory which will be `rm -rf` when dropped.
#[derive(Debug)]
pub struct TempDir {
    name: PathBuf,
}

impl TempDir {
    /// Create a new temporary directory
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
        let name = PathBuf::from(template.into_string()?);
        debug!("Temp dir: {}", name.display());
        Ok(TempDir { name })
    }

    /// Where is it?
    pub fn path(&self) -> &Path {
        &self.name
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_dir_all(&self.name) {
            error!(
                "Unable to remove temporary directory: {} : {}",
                self.name.display(),
                err
            );
        } else {
            debug!("Cleaned up: {}", self.name.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::write_file;

    #[test]
    fn test_tempdir() {
        let tdir = TempDir::new().unwrap();
        let dir = tdir.path().to_path_buf();

        assert!(dir.is_dir());

        let tfile = dir.join("test.txt");
        assert!(!tfile.is_file());
        write_file(&tfile, "Hello world").unwrap();
        assert!(tfile.is_file());

        drop(tdir);
        assert!(!tfile.is_file());
        assert!(!dir.is_dir());
    }
}
