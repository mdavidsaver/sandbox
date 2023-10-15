//! Direct manipulations of network configuration.  (eg. like `/sbin/ifconfig` or `/sbin/ip`)

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::net::{self, Ipv4Addr, UdpSocket};
use std::os::unix::prelude::*;
use std::ptr;

use log;

use super::err::{Error, Result};
use super::{ext, proc, util};

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

/// Wrap a `struct ifreq`.  Effectively an interface name.
#[derive(Copy, Clone)] // ifreq stores no pointers
struct IfReq(ext::ifreq);

impl IfReq {
    /// Fill in `ifreq::ifr_name`
    fn from_name<S: AsRef<str>>(name: S) -> Result<Self> {
        let rawname = name.as_ref().as_bytes().to_vec();
        let mut req = ext::ifreq::default();
        unsafe {
            if rawname.len() >= ::std::mem::size_of_val(&req.ifr_ifrn.ifrn_name) {
                Err(Error::TooLong)?;
            }
            // copy in iface with nil
            ptr::copy_nonoverlapping(
                rawname.as_ptr(),
                req.ifr_ifrn.ifrn_name.as_mut_ptr() as *mut u8,
                rawname.len(),
            );
            req.ifr_ifrn.ifrn_name[rawname.len()] = 0;
        }
        Ok(Self(req))
    }

    /// Make a `ioctl()` on the named interface
    unsafe fn ioctl<FD: AsRawFd>(&mut self, fd: FD, req: u32) -> Result<()> {
        let err = ext::ioctl(fd.as_raw_fd(), req as _, &mut self.0);
        if err != 0 {
            let mut raw = vec![0; ::std::mem::size_of_val(&self.0)];
            ptr::copy_nonoverlapping(
                &self.0 as *const _ as *const u8,
                raw.as_mut_ptr(),
                raw.len(),
            );
            Err(Error::last_os_error(format!(
                "ioctl({}, {:?}) -> {}",
                req, raw, err
            )))?;
        }
        Ok(())
    }
}

impl std::ops::Deref for IfReq {
    type Target = ext::ifreq;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for IfReq {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Network Interface Configurator.  A (small) sub-set of `/sbin/ifconfig`
pub struct IfConfig(UdpSocket);

impl IfConfig {
    /// Prepare to maniplate.  (allocates a "dummy" socket)
    pub fn new() -> Result<Self> {
        let sock =
            UdpSocket::bind("127.0.0.1:0").map_err(|e| Error::os("bind() ifconfig socket", e))?;
        Ok(Self(sock))
    }

    /// Map network interface name to numeric index
    pub fn ifindex<S: AsRef<str>>(&self, ifname: S) -> Result<u32> {
        let mut req = IfReq::from_name(ifname.as_ref())?;
        let ret = unsafe {
            req.ioctl(self.0.as_raw_fd(), ext::SIOCGIFINDEX)?;
            req.ifr_ifru.ifru_ivalue as u32
        };
        log::debug!("ifindex({:?}) -> {}", ifname.as_ref(), ret);
        Ok(ret)
    }

    /// Lookup interface flags bit mask
    pub fn ifflags<S: AsRef<str>>(&self, ifname: S) -> Result<u32> {
        let mut req = IfReq::from_name(ifname.as_ref())?;
        let ret = unsafe {
            req.ioctl(self.0.as_raw_fd(), ext::SIOCGIFFLAGS)?;
            req.ifr_ifru.ifru_flags as u32
        };
        log::debug!("ifflags({:?}) -> {}", ifname.as_ref(), ret);
        Ok(ret)
    }

    /// Overwrite interface flags bit mask
    pub fn set_ifflags<S: AsRef<str>>(&self, ifname: S, flags: u32) -> Result<()> {
        log::debug!("set_ifflags({:?}, {})", ifname.as_ref(), flags);
        let mut req = IfReq::from_name(ifname)?;
        unsafe {
            req.ifr_ifru.ifru_flags = flags as _;
            req.ioctl(self.0.as_raw_fd(), ext::SIOCSIFFLAGS)?;
            Ok(())
        }
    }

    /// Find "the" IPv4 address of the named interface.
    /// Unspecified (as in I don't know) how this behaves when more than one IPv4 address is assigned.
    pub fn address<S: AsRef<str>>(&self, ifname: S) -> Result<net::Ipv4Addr> {
        let mut req = IfReq::from_name(ifname.as_ref())?;
        let saddr = unsafe {
            req.ioctl(self.0.as_raw_fd(), ext::SIOCGIFADDR)?;
            if req.ifr_ifru.ifru_addr.sa_family != libc::AF_INET as libc::sa_family_t {
                Err(Error::NotIPv4)?;
            }
            let inaddr = &req.ifr_ifru.ifru_addr as *const _ as *const libc::sockaddr_in;
            (*inaddr).sin_addr.s_addr
        };
        let ret = net::Ipv4Addr::from(u32::from_be(saddr));
        log::debug!("address({:?}) -> {}", ifname.as_ref(), ret);
        Ok(ret)
    }

    /// Set "the" IPv4 address of the named interface.
    pub fn set_address<S: AsRef<str>>(&self, ifname: S, addr: net::Ipv4Addr) -> Result<()> {
        log::debug!("set_address({:?}, {})", ifname.as_ref(), addr);
        let iaddr = b2u32(addr.octets());
        let mut req = IfReq::from_name(ifname)?;
        unsafe {
            let inaddr = &mut req.ifr_ifru.ifru_addr as *mut _ as *mut libc::sockaddr_in;
            (*inaddr).sin_family = libc::AF_INET as libc::sa_family_t;
            (*inaddr).sin_port = 0;
            (*inaddr).sin_addr.s_addr = iaddr;
            req.ioctl(self.0.as_raw_fd(), ext::SIOCSIFADDR)?;
        }
        Ok(())
    }

    /// Create a soft ethernet bridge
    pub fn bridge_create<B: AsRef<str>>(&self, brname: B) -> Result<()> {
        log::debug!("bridge_create({:?})", brname.as_ref());
        let mut req = IfReq::from_name(brname)?;
        unsafe {
            // only the interface name is used
            req.ioctl(self.0.as_raw_fd(), ext::SIOCBRADDBR)?;
        }
        Ok(())
    }

    /// Add an interface to a soft ethernet bridge
    pub fn bridge_add<B: AsRef<str>, S: AsRef<str>>(&self, brname: B, ifname: S) -> Result<()> {
        let index = self.ifindex(ifname.as_ref())?;
        log::debug!(
            "bridge_add({:?}, {:?} ({}))",
            brname.as_ref(),
            ifname.as_ref(),
            index
        );
        let mut req = IfReq::from_name(brname)?;
        req.ifr_ifru.ifru_ivalue = index as _;
        unsafe {
            req.ioctl(self.0.as_raw_fd(), ext::SIOCBRADDIF)?;
        }
        Ok(())
    }
}

/// Management of a TUN or TAP interface
pub struct TunTap {
    name: String,
    fd: File,
}

impl TunTap {
    /// Create a new TAP interface.
    /// Lifetime is tied to the returned `TunTap`
    pub fn new<S: AsRef<str>>(name: S) -> Result<Self> {
        log::debug!("TunTap::new({:?})", name.as_ref());
        let name = name.as_ref().to_string();
        let fd = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/net/tun")
            .map_err(|e| Error::file("tuntap", "/dev/net/tun", e))?;

        let mut req = IfReq::from_name(&name)?;
        req.ifr_ifru.ifru_flags = (ext::IFF_TAP | ext::IFF_NO_PI) as _;
        unsafe {
            req.ioctl(fd.as_raw_fd(), ext::REAL_TUNSETIFF)?;
        }

        Ok(Self { name, fd })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// fork() a child process which will read and discard any packets
    /// set to this interface.  Keeps `IFF_RUNNING`
    pub fn handle_ignore(self) -> Result<proc::Proc> {
        let fd: OwnedFd = self.fd.into();
        let chld_fd = fd.as_raw_fd();

        util::set_cloexec(chld_fd, false)?;
        let err = proc::fork(|| -> std::io::Result<()> {
            let mut file = unsafe { File::from_raw_fd(chld_fd) };
            let mut buf = vec![0; 0x10000];
            loop {
                file.read(&mut buf)?;
            }
        });
        util::set_cloexec(chld_fd, true)?;
        let chld = err?;

        Ok(chld)
    }
}

/// Bring the "lo" interface UP with 127.0.0.1
pub fn configure_lo() -> Result<()> {
    log::debug!("Setup loopback interface");

    let conf = IfConfig::new()?;

    log::debug!("Set lo address");
    conf.set_address(LOOPBACK, Ipv4Addr::LOCALHOST)?;

    let flags = conf.ifflags(LOOPBACK)?;
    if 0 == (flags & ext::IFF_UP) {
        log::debug!("Bring lo UP");
        conf.set_ifflags(LOOPBACK, ext::IFF_UP | flags)?;
    }

    Ok(())
}

/// A "dummy" software ethernet bridge
pub struct Bridge(proc::Proc);

/// Add a broadcast capable bridge with a dummy tun interface.
pub fn dummy_bridge() -> Result<Bridge> {
    log::debug!("Setup dummy bridge");

    let conf = IfConfig::new()?;

    conf.bridge_create("br0")?;

    let tun = TunTap::new("tap0")?;

    conf.bridge_add("br0", tun.name())?;

    let brf = conf.ifflags("br0")?;
    conf.set_address("br0", Ipv4Addr::new(192, 168, 1, 1))?;
    conf.set_ifflags("br0", brf | ext::IFF_UP)?;

    let brf = conf.ifflags(tun.name())?;
    conf.set_ifflags(tun.name(), brf | ext::IFF_UP)?;
    // TODO: why does tap0 have an ipv6 address?

    Ok(Bridge(tun.handle_ignore()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lo_flags() {
        let conf = IfConfig::new().unwrap();
        let flags = conf.ifflags(LOOPBACK).expect("flags");
        assert!((flags & ext::IFF_LOOPBACK) != 0, "flags {}", flags);
    }

    #[test]
    fn lo_addr() {
        let conf = IfConfig::new().unwrap();
        let addr = conf.address(LOOPBACK).expect("address");
        assert_eq!(addr, net::Ipv4Addr::LOCALHOST);
    }

    #[test]
    fn lo_index() {
        let conf = IfConfig::new().unwrap();
        let idx = conf.ifindex(LOOPBACK).expect("address");
        // TODO: is this actually certain?
        assert_eq!(idx, 1);
    }
}
