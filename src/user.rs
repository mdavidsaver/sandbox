//! Wrappers for UID and GID syscalls

use libc;

use super::err::{Error, Result};

pub fn getuid() -> libc::uid_t {
    unsafe { libc::getuid() }
}

pub fn geteuid() -> libc::uid_t {
    unsafe { libc::geteuid() }
}

pub fn setuid(id: libc::uid_t) -> Result<()> {
    unsafe {
        if 0 != libc::setuid(id) {
            return Err(Error::last_os_error("setuid"));
        }
    }
    Ok(())
}

pub fn seteuid(id: libc::uid_t) -> Result<()> {
    unsafe {
        if 0 != libc::seteuid(id) {
            return Err(Error::last_os_error("seteuid"));
        }
    }
    Ok(())
}

pub fn getgid() -> libc::gid_t {
    unsafe { libc::getgid() }
}

pub fn getegid() -> libc::gid_t {
    unsafe { libc::getegid() }
}

pub fn setgid(id: libc::gid_t) -> Result<()> {
    unsafe {
        if 0 != libc::setgid(id) {
            return Err(Error::last_os_error("setgid"));
        }
    }
    Ok(())
}

pub fn setegid(id: libc::gid_t) -> Result<()> {
    unsafe {
        if 0 != libc::setegid(id) {
            return Err(Error::last_os_error("setegid"));
        }
    }
    Ok(())
}
