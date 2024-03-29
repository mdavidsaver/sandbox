use std::{env, process};

use libc;
use log::debug;

use sandbox::container::ContainerHooks;
use sandbox::{net, util};
use sandbox::{runc, Error};

struct NoNet {
    args: Vec<String>,
}

impl ContainerHooks for NoNet {
    fn unshare(&self) -> Result<(), Error> {
        debug!("child unshare()");
        util::unshare(libc::CLONE_NEWNET)?;
        Ok(())
    }

    fn setup_priv(&self) -> Result<(), Error> {
        // setup loopback only

        net::configure_lo()?;

        Ok(())
    }

    fn setup(&self) -> Result<(), Error> {
        debug!("EXEC {:?}", &self.args[0..]);

        util::Exec::new(&self.args[0])?
            .args(&self.args[0..])?
            .exec()?;

        Ok(())
    }
}

fn main() -> Result<(), Error> {
    sandbox::logging::setup().unwrap();

    let rawargs = env::args().collect::<Vec<String>>();
    if rawargs.len() <= 1 {
        eprintln!("Usage: {} <cmd> [args ...]", rawargs[0]);
        process::exit(1);
    }

    process::exit(runc(&NoNet {
        args: rawargs[1..].to_vec(),
    })?);
}
