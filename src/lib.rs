//! sandbox - Limited Linux containers
//!
//! Installs executables:
//! - hidehome - Run command with (apparently) empty $HOME
//! - isolate  - Run command with only $PWD writable, and not network access.
//! - nonet    - Run command with no network access

mod err;

mod ext;

mod capability;

pub mod fs;
pub mod net;
mod proc;
pub mod tempdir;
mod user;

pub mod container;
pub use container::runc;
pub use container::ContainerHooks;
pub use container::{Error, Result};

pub mod util;
