use std::net::Ipv4Addr;

use libc;
use log::debug;

use super::Error;
use super::container::{ContainerHooks};
use super::{ext, net, util};

pub struct NoNet {
    args: Vec<String>,
}

impl NoNet {
    pub fn new<A, B, I>(cmd: A, args: I) -> NoNet
    where
        A: AsRef<str>,
        B: AsRef<str>,
        I: IntoIterator<Item = B>,
    {
        let mut cmd = vec![cmd.as_ref().to_string()];
        for arg in args {
            cmd.push(arg.as_ref().to_string());
        }
        NoNet {
            args: cmd,
        }
    }
}

impl ContainerHooks for NoNet {
    fn unshare(&self) -> Result<(), Error> {
        debug!("child unshare()");
        util::unshare(libc::CLONE_NEWNET)?;
        Ok(())
    }

    fn setup_priv(&self) -> Result<(), Error> {
        // setup loopback only

        let lo = net::IFaceV4::new(net::LOOPBACK)?;

        debug!("Set lo address");
        lo.set_address(Ipv4Addr::LOCALHOST)?;

        let flags = lo.flags()?;
        if 0==(flags&ext::IFF_UP) {
            debug!("Bring lo UP");
            lo.set_flags(ext::IFF_UP | flags)?;
        }

        Ok(())
    }

    fn setup(&self) -> Result<(), Error> {
        debug!("EXEC {:?}", &self.args[1..]);

        util::Exec::new(&self.args[1])?
            .args(&self.args[1..])?
            .exec()?;

        Ok(())
    }
}
