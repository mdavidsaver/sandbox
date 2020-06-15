use std::{env, process};

use sandbox::nonet::NoNet;
use sandbox::{runc, Error};

fn main() -> Result<(), Error> {
    env_logger::init();

    let rawargs = env::args().collect::<Vec<String>>();
    if rawargs.len() <= 1 {
        eprintln!("Usage: {} <cmd> [args ...]", rawargs[0]);
        process::exit(1);
    }

    runc(&NoNet::new(&rawargs[1], &rawargs[1..]))?;

    Ok(())
}
