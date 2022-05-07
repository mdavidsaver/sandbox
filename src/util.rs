use std::ffi::CString;
use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};

use std::os::unix::io::FromRawFd;
use std::os::unix::fs::MetadataExt;

use libc;

use log::debug;

pub use super::capability::*;
pub use super::container::*;
use super::err::{Error, Result};
pub use super::proc::*;
pub use super::user::*;

fn path2cstr<P: AsRef<Path>>(path: P) -> Result<CString> {
    let ret = CString::new(path.as_ref().to_string_lossy().as_ref())?;
    Ok(ret)
}

pub fn write_file<P: AsRef<Path>, S: AsRef<[u8]>>(name: P, buf: S) -> Result<()> {
    debug!("write_file({:?}, ...)", name.as_ref().display());
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(name.as_ref())
        .map_err(|e| Error::file("open", name.as_ref(), e))?;
    file.write_all(buf.as_ref())
        .map_err(|e| Error::file("write", name.as_ref(), e))
}

pub fn mkdir<S: AsRef<Path>>(name: S) -> Result<PathBuf> {
    debug!("mkdir({:?})", name.as_ref().display());
    fs::create_dir(name.as_ref()).map_err(|e| Error::file("mkdir", name.as_ref(), e))?;
    Ok(name.as_ref().to_path_buf())
}

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
            let st = sdir.metadata().map_err(|e| Error::file("stat()",sdir,e) )?;
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

pub fn rmdir<S: AsRef<Path>>(name: S) -> Result<()> {
    debug!("rmdir({:?})", name.as_ref().display());
    fs::remove_dir(name.as_ref()).map_err(|e| Error::file("rmdir", name.as_ref(), e))
}

pub fn chown<S: AsRef<Path>>(path: S, uid: libc::uid_t, gid: libc::gid_t) -> Result<()> {
    debug!("chown({:?}, {}, {})", path.as_ref().display(), uid, gid);
    let rawname = path2cstr(&path)?;
    unsafe {
        if libc::chown(rawname.as_ptr(), uid, gid) == 0 {
            Ok(())
        } else {
            Err(Error::last_file_error("chown", path))
        }
    }
}

pub fn chmod<S: AsRef<Path>>(path: S, mode: u32) -> Result<()> {
    debug!("chmod({:?}, {:#o})", path.as_ref().display(), mode);
    let rawname = path2cstr(&path)?;
    unsafe {
        if libc::chmod(rawname.as_ptr(), mode as libc::mode_t) == 0 {
            Ok(())
        } else {
            Err(Error::last_file_error("chmod", path))
        }
    }
}

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

pub fn unshare(flags: libc::c_int) -> Result<()> {
    debug!("unshare(0x{:x})", flags);
    unsafe {
        if libc::unshare(flags) != 0 {
            return Err(Error::last_os_error("unshare"));
        }
    }
    Ok(())
}

pub fn mount<A, B, C>(src: A, target: B, fstype: C, flags: libc::c_ulong) -> Result<()>
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
) -> Result<()>
where
    A: AsRef<Path>,
    B: AsRef<Path>,
    C: AsRef<Path>,
    D: AsRef<Path>,
{
    let csrc = path2cstr(&src)?;
    let ctarget = path2cstr(&target)?;
    let cfstype = path2cstr(&fstype)?;
    let cdata = path2cstr(&data)?;
    debug!(
        "mount({:?},{:?},{:?},0x{:x},{:?})",
        csrc, ctarget, cfstype, flags, cdata
    );
    unsafe {
        if 0 != libc::mount(
            csrc.as_ptr() as *const libc::c_char,
            ctarget.as_ptr() as *const libc::c_char,
            cfstype.as_ptr() as *const libc::c_char,
            flags,
            cdata.as_ptr() as *const libc::c_void,
        ) {
            Err(Error::last_os_error(format!(
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

pub fn create_file<P: AsRef<Path>>(fname: P, perm: libc::mode_t) -> Result<fs::File> {
    let rawname = path2cstr(&fname)?;
    let fd;
    unsafe {
        fd = libc::creat(rawname.as_ptr(), perm);
        if fd < 0 {
            Err(Error::last_file_error("creat", fname))
        } else {
            Ok(fs::File::from_raw_fd(fd))
        }
    }
}

pub fn umount_lazy<P: AsRef<Path>>(path: P) -> Result<()> {
    debug!("umount({:?})", path.as_ref().display());
    let rawname = path2cstr(&path)?;
    unsafe {
        let ret = libc::umount2(rawname.as_ptr(), libc::MNT_DETACH);
        if ret == 0 {
            Ok(())
        } else {
            Err(Error::last_file_error("umount2", path))
        }
    }
}

pub fn maybe_umount_lazy<P: AsRef<Path>>(path: P) -> Result<bool> {
    debug!("umount({:?})", path.as_ref().display());
    let rawname = path2cstr(&path)?;
    unsafe {
        let ret = libc::umount2(rawname.as_ptr(), libc::MNT_DETACH);
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
}

pub fn pivot_root<A: AsRef<Path>, B: AsRef<Path>>(new_root: A, old_root: B) -> Result<()> {
    debug!(
        "pivot_root({:?}, {:?})",
        new_root.as_ref().display(),
        old_root.as_ref().display()
    );
    let rawnew = path2cstr(&new_root)?;
    let rawold = path2cstr(&old_root)?;
    unsafe {
        // no libc wrapper
        let ret = libc::syscall(
            libc::SYS_pivot_root,
            rawnew.as_ptr() as *const libc::c_char,
            rawold.as_ptr() as *const libc::c_char,
        );
        if ret == 0 {
            Ok(())
        } else {
            Err(Error::last_file_error("pivot_root", new_root))
        }
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
