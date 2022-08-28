use std::net::{self, Ipv4Addr};
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::ptr;

use log;

use super::ext;

pub use ext::IFF_UP;

use super::err::{Error, Result};

pub const LOOPBACK: &str = "lo";

// for lack of Ipv4Addr::integer() -> u32
fn b2u32(b: [u8; 4]) -> u32 {
    let mut ret = b[3] as u32;
    ret <<= 8;
    ret |= b[2] as u32;
    ret <<= 8;
    ret |= b[1] as u32;
    ret <<= 8;
    ret |= b[0] as u32;
    ret
}

pub struct IFaceV4 {
    req: ext::ifreq,
    sock: OwnedFd,
}

impl IFaceV4 {
    pub fn new<S: AsRef<str>>(name: S) -> Result<IFaceV4> {
        let rawname = name.as_ref().as_bytes().to_vec();
        let mut req = ext::ifreq::default();
        let sock;
        unsafe {
            if rawname.len() >= ::std::mem::size_of_val(&req.ifr_ifrn.ifrn_name) {
                Err(Error::TooLong)?;
            }

            let ret = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
            if ret < 0 {
                Err(Error::last_os_error("socket()"))?;
            }
            sock = OwnedFd::from_raw_fd(ret);

            // copy in iface with nil
            ptr::copy_nonoverlapping(
                rawname.as_ptr(),
                req.ifr_ifrn.ifrn_name.as_mut_ptr() as *mut u8,
                rawname.len(),
            );
            req.ifr_ifrn.ifrn_name[rawname.len()] = 0;
        }
        Ok(IFaceV4 { req, sock })
    }

    pub fn flags(&self) -> Result<u32> {
        let req = self.req.clone();
        unsafe {
            if ext::ioctl(
                self.sock.as_raw_fd(),
                ext::SIOCGIFFLAGS as ::std::os::raw::c_ulong,
                &req,
            ) != 0
            {
                Err(Error::last_os_error("ioctl(SIOCGIFFLAGS)"))?;
            }
            Ok(req.ifr_ifru.ifru_flags as u32)
        }
    }

    pub fn set_flags(&self, flags: u32) -> Result<()> {
        let mut req = self.req.clone();
        unsafe {
            req.ifr_ifru.ifru_flags = flags as libc::c_short;
            if ext::ioctl(
                self.sock.as_raw_fd(),
                ext::SIOCSIFFLAGS as ::std::os::raw::c_ulong,
                &req,
            ) != 0
            {
                Err(Error::last_os_error("ioctl(SIOCSIFFLAGS)"))?;
            }
            Ok(())
        }
    }

    pub fn address(&self) -> Result<net::Ipv4Addr> {
        let req = self.req.clone();
        unsafe {
            if ext::ioctl(
                self.sock.as_raw_fd(),
                ext::SIOCGIFADDR as ::std::os::raw::c_ulong,
                &req,
            ) != 0
            {
                Err(Error::last_os_error("ioctl(SIOCGIFADDR)"))?;
            }
            if req.ifr_ifru.ifru_addr.sa_family != libc::AF_INET as libc::sa_family_t {
                Err(Error::NotIPv4)?;
            }
            let inaddr = &req.ifr_ifru.ifru_addr as *const _ as *const libc::sockaddr_in;
            Ok(net::Ipv4Addr::from(u32::from_be((*inaddr).sin_addr.s_addr)))
        }
    }

    pub fn set_address(&self, addr: net::Ipv4Addr) -> Result<()> {
        let iaddr = b2u32(addr.octets());
        let mut req = self.req.clone();
        unsafe {
            let mut inaddr = &mut req.ifr_ifru.ifru_addr as *mut _ as *mut libc::sockaddr_in;
            (*inaddr).sin_family = libc::AF_INET as libc::sa_family_t;
            (*inaddr).sin_port = 0;
            (*inaddr).sin_addr.s_addr = iaddr;
            if ext::ioctl(
                self.sock.as_raw_fd(),
                ext::SIOCSIFADDR as ::std::os::raw::c_ulong,
                &req,
            ) != 0
            {
                Err(Error::last_os_error("ioctl(SIOCSIFADDR)"))?;
            }
        }
        Ok(())
    }
}

/// Bring the "lo" interface UP with 127.0.0.1
pub fn configure_lo() -> Result<()> {
    log::debug!("Setup loopback interface");

    let lo = IFaceV4::new(LOOPBACK)?;

    log::debug!("Set lo address");
    lo.set_address(Ipv4Addr::LOCALHOST)?;

    let flags = lo.flags()?;
    if 0 == (flags & IFF_UP) {
        log::debug!("Bring lo UP");
        lo.set_flags(IFF_UP | flags)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lo_flags() {
        let iface = IFaceV4::new(LOOPBACK).expect("Can't make lo");

        let flags = iface.flags().expect("flags");
        assert!((flags & ext::IFF_LOOPBACK) != 0, "flags {}", flags);
    }

    #[test]
    fn lo_addr() {
        let iface = IFaceV4::new(LOOPBACK).expect("Can't make lo");
        let addr = iface.address().expect("address");
        assert_eq!(addr, net::Ipv4Addr::LOCALHOST);
    }
}
