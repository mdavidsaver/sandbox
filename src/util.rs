use std::ffi::CString;
use std::fs;
use std::io::Write;
use std::net::TcpStream;
use std::path::Path;

use std::os::unix::io::FromRawFd;

use libc;

use log::debug;

pub use super::capability::*;
pub use super::container::*;
use super::err::{Error, Result};
pub use super::proc::*;
pub use super::user::*;

pub fn write_file<P: AsRef<Path>>(name: P, buf: &[u8]) -> Result<()> {
    debug!("write_file({}, ...)", name.as_ref().display());
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(name.as_ref())
        .map_err(|e| Error::file("open", name.as_ref(), e))?;
    file.write_all(buf)
        .map_err(|e| Error::file("write", name.as_ref(), e))
}

pub fn mkdirs<S: AsRef<Path>>(name: S) -> Result<()> {
    debug!("mkdirs({})", name.as_ref().display());
    fs::create_dir_all(name.as_ref()).map_err(|e| Error::file("mkdirs", name.as_ref(), e))
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
    let rawname = CString::new(fname.as_ref().to_string_lossy().as_ref())?;
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
