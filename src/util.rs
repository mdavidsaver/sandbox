use std::ffi::CString;
use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::os::unix::prelude::*;
use std::path::{Path, PathBuf};

use std::os::unix::fs::MetadataExt;
use std::os::unix::io::FromRawFd;

use libc;

use log::debug;

pub use super::capability::*;
use super::err::{Error, Result};
pub use super::proc::*;
pub use super::user::*;

/// Allocate a `CString` from the given path.
fn str2cstr<S: AsRef<str>>(s: S) -> Result<CString> {
    let ret = CString::new(s.as_ref())?;
    Ok(ret)
}

/// Allocate a `CString` from the given path.
fn path2cstr<P: AsRef<Path>>(path: P) -> Result<CString> {
    str2cstr(path.as_ref().to_string_lossy())
}

/// Create a file, and write the provided bytes
pub fn write_file<P: AsRef<Path>, S: AsRef<[u8]>>(name: P, buf: S) -> Result<()> {
    debug!("write_file({:?}, ...)", name.as_ref().display());
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(name.as_ref())
        .map_err(|e| Error::file("open", name.as_ref(), e))?
        .write_all(buf.as_ref())
        .map_err(|e| Error::file("write", name.as_ref(), e))
}

/// Wraps `mkdir()`.  Only attempts to create the leaf
pub fn mkdir<S: AsRef<Path>>(name: S) -> Result<PathBuf> {
    debug!("mkdir({:?})", name.as_ref().display());
    fs::create_dir(name.as_ref()).map_err(|e| Error::file("mkdir", name.as_ref(), e))?;
    Ok(name.as_ref().to_path_buf())
}

/// `mkdir()` for the leaf directory and all parents.  eg. `install -d /some/dirs`.
pub fn mkdirs<S: AsRef<Path>>(name: S) -> Result<PathBuf> {
    debug!("mkdirs({:?})", name.as_ref().display());
    fs::create_dir_all(name.as_ref()).map_err(|e| Error::file("mkdirs", name.as_ref(), e))?;
    Ok(name.as_ref().to_path_buf())
}

/// Create directory /B/A with same ownership and permissions as /A
pub fn clonedirs<A: AsRef<Path>, B: AsRef<Path>>(src: A, target: B) -> Result<()> {
    assert!(src.as_ref().is_absolute(), "{:?}", src.as_ref());
    assert!(target.as_ref().is_absolute(), "{:?}", target.as_ref());
    // iterate from root to leaf
    for sdir in src.as_ref().ancestors().collect::<Vec<_>>().iter().rev() {
        let tg = target.as_ref().join(sdir.strip_prefix("/").unwrap());
        if !tg.exists() {
            debug!("clone path {}", tg.display());
            let st = sdir
                .metadata()
                .map_err(|e| Error::file("stat()", sdir, e))?;
            if st.is_dir() {
                drop(mkdir(&tg)?);
            } else {
                fs::write(&tg, b"").map_err(|e| Error::file("write", &tg, e))?;
            }
            chmod(&tg, st.mode() & 0o7777)?;
            chown(&tg, st.uid(), st.gid())?;
        }
    }
    Ok(())
}

/// Wraps `rmdir ...`
pub fn rmdir<S: AsRef<Path>>(name: S) -> Result<()> {
    debug!("rmdir({:?})", name.as_ref().display());
    fs::remove_dir(name.as_ref()).map_err(|e| Error::file("rmdir", name.as_ref(), e))
}

/// Wraps `chown()`
pub fn chown<S: AsRef<Path>>(path: S, uid: libc::uid_t, gid: libc::gid_t) -> Result<()> {
    debug!("chown({:?}, {}, {})", path.as_ref().display(), uid, gid);
    if unsafe { libc::chown(path2cstr(&path)?.as_ptr(), uid, gid) } == 0 {
        Ok(())
    } else {
        Err(Error::last_file_error("chown", path))
    }
}

/// Wraps `chmod()`
pub fn chmod<S: AsRef<Path>>(path: S, mode: u32) -> Result<()> {
    debug!("chmod({:?}, {:#o})", path.as_ref().display(), mode);
    if unsafe { libc::chmod(path2cstr(&path)?.as_ptr(), mode as libc::mode_t) } == 0 {
        Ok(())
    } else {
        Err(Error::last_file_error("chmod", path))
    }
}

/// Create a pair of connected stream sockets.  Will be `SOCK_STREAM`.  May not actually be `AF_INET` or `AF_INET6`.
pub fn socketpair() -> Result<(TcpStream, TcpStream)> {
    let mut fds = vec![0, 2];
    unsafe {
        if 0 != libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) {
            return Err(Error::last_os_error("socketpair"));
        }
        Ok((
            TcpStream::from_raw_fd(fds[0]),
            TcpStream::from_raw_fd(fds[1]),
        ))
    }
}

/// Wraps `unshare()`
pub fn unshare(flags: libc::c_int) -> Result<()> {
    debug!("unshare(0x{:x})", flags);
    if unsafe { libc::unshare(flags) } != 0 {
        return Err(Error::last_os_error("unshare"));
    }
    Ok(())
}

/// Wraps `mount()`
pub fn mount<A, B, C>(src: A, target: B, fstype: C, flags: libc::c_ulong) -> Result<()>
where
    A: AsRef<Path>,
    B: AsRef<Path>,
    C: AsRef<str>,
{
    mount_with_data(src, target, fstype, flags, "")
}

/// Wraps `mount()`
pub fn mount_with_data<A, B, C, D>(
    src: A,
    target: B,
    fstype: C,
    flags: libc::c_ulong,
    data: D,
) -> Result<()>
where
    A: AsRef<Path>,
    B: AsRef<Path>,
    C: AsRef<str>,
    D: AsRef<str>,
{
    debug!(
        "mount({:?},{:?},{:?},0x{:x},{:?})",
        src.as_ref().display(),
        target.as_ref().display(),
        fstype.as_ref(),
        flags,
        data.as_ref()
    );
    if 0 != unsafe {
        libc::mount(
            path2cstr(&src)?.as_ptr(),
            path2cstr(&target)?.as_ptr(),
            str2cstr(&fstype)?.as_ptr() as *const _,
            flags,
            str2cstr(&data)?.as_ptr() as *const _,
        )
    } {
        Err(Error::last_os_error(format!(
            "mount src={:?} target={:?} fs={:?} flags=0x{:x} data=",
            src.as_ref(),
            target.as_ref(),
            fstype.as_ref(),
            flags
        )))?;
    }
    Ok(())
}

/// Wraps `umount2(..., MNT_DETACH)` to remove a mount from the current namespace,
/// but not necessarily from others.
pub fn umount_lazy<P: AsRef<Path>>(path: P) -> Result<()> {
    debug!("umount({:?})", path.as_ref().display());
    let ret = unsafe { libc::umount2(path2cstr(&path)?.as_ptr(), libc::MNT_DETACH) };
    if ret == 0 {
        Ok(())
    } else {
        Err(Error::last_file_error("umount2", path))
    }
}

/// Try to `umount_lazy()`
pub fn maybe_umount_lazy<P: AsRef<Path>>(path: P) -> Result<bool> {
    debug!("umount({:?})", path.as_ref().display());
    let ret = unsafe { libc::umount2(path2cstr(&path)?.as_ptr(), libc::MNT_DETACH) };
    if ret == 0 {
        debug!("  Success");
        Ok(true)
    } else if std::io::Error::last_os_error().raw_os_error().unwrap() == libc::EINVAL {
        debug!("  Nope");
        Ok(false)
    } else {
        Err(Error::last_file_error("umount2", path))
    }
}

unsafe fn sys_pivot_root(
    new_root: *const libc::c_char,
    old_root: *const libc::c_char,
) -> libc::c_int {
    libc::syscall(libc::SYS_pivot_root, new_root, old_root) as _
}

/// Wraps `pivot_root()`
pub fn pivot_root<A: AsRef<Path>, B: AsRef<Path>>(new_root: A, old_root: B) -> Result<()> {
    debug!(
        "pivot_root({:?}, {:?})",
        new_root.as_ref().display(),
        old_root.as_ref().display()
    );
    // no libc wrapper
    let ret = unsafe {
        sys_pivot_root(
            path2cstr(&new_root)?.as_ptr(),
            path2cstr(&old_root)?.as_ptr(),
        )
    };
    if ret == 0 {
        Ok(())
    } else {
        Err(Error::last_file_error("pivot_root", new_root))
    }
}

/// Maniplate the `O_CLOEXEC` bit on the provided file descriptor.
pub fn set_cloexec<F: AsRawFd>(fd: F, v: bool) -> Result<()> {
    let fdn = fd.as_raw_fd();
    let mut cur = unsafe { libc::fcntl(fdn, libc::F_GETFD) };
    if cur < 0 {
        return Err(Error::last_os_error("F_GETFD"));
    }
    if v {
        cur |= libc::O_CLOEXEC;
    } else {
        cur &= !libc::O_CLOEXEC;
    }
    let err = unsafe { libc::fcntl(fdn, libc::F_SETFD, cur) };
    if err < 0 {
        return Err(Error::last_os_error("F_SETFD"));
    }
    Ok(())
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
        set_cloexec(a.as_raw_fd(), true).unwrap();
        set_cloexec(b.as_raw_fd(), true).unwrap();

        a.write_all("msg".as_bytes()).unwrap();
        let mut buf = vec![0; 4];
        let n = b.read(&mut buf).unwrap();
        assert_eq!(n, 3);
        assert_eq!(&buf[0..3], "msg".as_bytes());
    }

    #[test]
    fn test_cstr() {
        let cstr = path2cstr("/some/path").unwrap();
        assert_eq!(cstr.to_str().unwrap(), "/some/path");
    }
}
