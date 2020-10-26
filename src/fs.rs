use std::fs;
use std::path::{Path, PathBuf};

use std::os::unix::fs::MetadataExt;

use super::err::{Error, Result};

/// Find the (parent) directory which is a mount point for this file/directory.
///
/// See src/find-mount-point.c in GNU coreutils
pub fn find_mount_point<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
    let path = path
        .as_ref()
        .canonicalize()
        .map_err(|e| Error::file("canonicalize", &path, e))?;
    let s = fs::metadata(&path).map_err(|e| Error::file("metadata", &path, e))?;

    //
    let mut dir = if s.file_type().is_dir() {
        &path
    } else {
        // canonicalize'd files always have a "parent"
        path.parent().unwrap()
    };

    loop {
        if let Some(next) = dir.parent() {
            let nexts = fs::metadata(&next).map_err(|e| Error::file("metadata", &next, e))?;
            // assume nexts.ftype==FileType::Dir
            if s.dev() != nexts.dev() || s.ino() == nexts.ino() {
                // parent is a different mount point
                return Ok(dir.to_path_buf());
            }
            dir = next;
        } else {
            // reached root, assumed to be a mountpoint
            return Ok(dir.to_path_buf());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cwd() {
        let cwd = std::env::current_dir().unwrap();
        let ret = find_mount_point(&cwd);
        ret.unwrap();
    }

    #[test]
    fn test_root() {
        let ret = find_mount_point(&"/").unwrap();
        assert_eq!(ret, Path::new(&"/"));
    }

    #[test]
    fn test_empty() {
        let ret = find_mount_point(&"");
        assert!(ret.is_err(), "{:?}", ret);
    }
}
