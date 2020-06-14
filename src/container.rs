use std::collections::BTreeMap;
use std::error;
use std::io::{self, Read, Write};
use std::net;
use std::process::{exit, Command};

use log::debug;

use libc;

use super::proc::{fork, Fork};
use super::{ext, util};

pub use super::proc::Proc;

pub type Error = Box<dyn error::Error + 'static>;

/// Container lifecycle hooks
#[allow(unused_variables)]
pub trait ContainerHooks {
    /// Called in parent process before child is forked
    fn at_start(&self) -> Result<(), Error> {
        Ok(())
    }
    /// Called from child process when time to unshare()
    fn unshare(&self) -> Result<(), Error> {
        Ok(())
    }
    /// Called from parent when time to set child uid/gid_map.
    fn set_id_map(&self, pid: &Proc) -> Result<(), Error> {
        Ok(())
    }
    /// Called from grandchild with full privlage (all capabilities)
    fn setup_priv(&self) -> Result<(), Error> {
        Ok(())
    }
    /// Called from grandchild with final privlage (no capabilities)
    fn setup(&self) -> Result<(), Error> {
        Ok(())
    }
}

fn handle_parent<H: ContainerHooks>(
    hooks: &H,
    mut pid: Proc,
    mut tochild: net::TcpStream,
) -> Result<i32, Error> {
    // wait for child to unshare()
    let mut msg = vec![0; 1];
    tochild.read_exact(&mut msg).or_else(|err| {
        if err.kind() == io::ErrorKind::UnexpectedEof {
            msg[0] = '!' as u8;
            Ok(())
        } else {
            Err(err)
        }
    })?;

    if (msg[0] as char) == '.' {
        hooks.set_id_map(&pid)?;
        //.annotate("HOOK set_id_map")?;
        // notify child to proceed
        tochild.write_all(".".as_bytes())?;
    } else {
        debug!("Child sent err msg {:?}", msg);
    }

    debug!("Parent park");
    // drop SUID-ness
    util::setegid(util::getgid())?;
    util::seteuid(util::getuid())?;
    // wait for child to exit
    Ok(pid.park()?)
}

fn handle_child<H: ContainerHooks>(hooks: &H, mut toparent: net::TcpStream) -> Result<(), Error> {
    hooks
        .unshare()
        //.annotate("HOOK unshare()")
        .or_else(|err| {
            if let Some(_err) = err
                .source()
                .and_then(|err| err.downcast_ref::<io::Error>())
                .filter(|err| err.kind() == io::ErrorKind::PermissionDenied)
            {
                eprintln!("Error: Insufficient permission to unshare.");
                eprintln!("");
                eprintln!("       Must either have root (uid 0), CAP_SYS_ADMIN,");
                eprintln!("       or enable non-privlaged user namespaces by eg.");
                eprintln!("");
                eprintln!("       echo 1 > /proc/sys/kernel/unprivileged_userns_clone");
                exit(1);
            }
            // ask parent to setup uid/gid maps
            toparent.write_all("X".as_bytes())?;
            Err(err)
        })?;

    // ask parent to setup uid/gid maps
    toparent.write_all(".".as_bytes())?;

    // wait for parent
    let mut msg = vec![0; 1];
    toparent.read_exact(&mut msg)?;
    drop(toparent);
    debug!("child continue");

    match fork()? {
        Fork::Parent(mut pid) => {
            debug!("Forked Grandchild {}", pid);
            debug!("Child park");
            // drop SUID-ness
            util::setegid(util::getgid())?;
            util::seteuid(util::getuid())?;
            // wait for child to exit
            exit(pid.park()?);
        }
        Fork::Child => match handle_grandchild(hooks) {
            Ok(()) => exit(0),
            Err(err) => {
                eprintln!("Child error: {}", err);
                exit(1)
            }
        },
    }
}

fn handle_grandchild<H: ContainerHooks>(hooks: &H) -> Result<(), Error> {
    debug!("Grandchild");

    // clear SUID-ness
    util::setegid(util::getgid())?;
    util::seteuid(util::getuid())?;
    // effective capabilities have been cleared.  permitted remain unset
    // re-activate all capabilities
    util::Cap::current()?.activate().update()?;

    debug!(
        "Perms uid {},{} gid {},{}",
        util::getuid(),
        util::geteuid(),
        util::getgid(),
        util::getegid()
    );
    debug!("Cap {}", util::Cap::current()?);

    hooks.setup_priv()?;

    // drop all capabilities, effective, permitted, and inheritable
    util::Cap::current()?.clear().update()?;
    debug!("Drop caps");
    debug!("Cap {}", util::Cap::current()?);

    hooks.setup()?;
    Ok(())
}

/// Launch container with given hooks.
pub fn runc<H: ContainerHooks>(hooks: &H) -> Result<i32, Error> {
    // communications between parent and child to coordinate SetIdMap()

    hooks.at_start()?;
    //.annotate("HOOK at_start()")?;

    let (parent, child) = util::socketpair()?;

    match fork()? {
        Fork::Parent(pid) => {
            drop(child);
            debug!("Forked Child {}", pid);
            handle_parent(hooks, pid, parent)
        }
        Fork::Child => {
            drop(parent);
            match handle_child(hooks, child) {
                Ok(()) => exit(0),
                Err(err) => {
                    eprintln!("Child error: {}", err);
                    exit(1)
                }
            }
        }
    }
}

pub struct IdMap {
    pid: libc::pid_t,
    isuid: bool,
    map: BTreeMap<u32, (u32, u32)>,
}

impl IdMap {
    pub fn new_uid(pid: libc::pid_t) -> IdMap {
        IdMap {
            pid,
            isuid: true,
            map: BTreeMap::new(),
        }
    }

    pub fn new_gid(pid: libc::pid_t) -> IdMap {
        IdMap {
            pid,
            isuid: false,
            map: BTreeMap::new(),
        }
    }

    pub fn add(&mut self, start: u32, end: u32, count: u32) -> &mut Self {
        self.map.insert(start, (end, count));
        self
    }

    fn map_args<'a>(&'a self) -> Vec<String> {
        self.map
            .iter()
            .map(|(start, (end, count))| vec![start, end, count])
            .flatten()
            .map(|e| format!("{}", e))
            .collect()
    }

    fn map_file(&self) -> String {
        // emit mapping as lines
        //   start# end# count#\n
        self.map
            .iter()
            .map(|(start, (end, count))| format!("{} {} {}\n", start, end, count))
            .fold(String::new(), |mut a, b| {
                a += &b;
                a
            })
    }

    pub fn write(&self) -> Result<(), Error> {
        let caps = util::Cap::current()?;

        if self.isuid && caps.effective(ext::CAP_SETUID) {
            // directly write uid_map
            util::write_file(
                format!("/proc/{}/uid_map", self.pid),
                self.map_file().as_bytes(),
            )?;
        } else if !self.isuid && caps.effective(ext::CAP_SETGID) {
            // directly write gid_map
            util::write_file(
                format!("/proc/{}/gid_map", self.pid),
                self.map_file().as_bytes(),
            )?;
        } else {
            // call newuidmap or newgidmap
            let cmd = if self.isuid { "newuidmap" } else { "newgidmap" };
            let args = self.map_args();
            debug!("run: {} {} {:?}", cmd, self.pid, &args);

            Command::new(cmd)
                .arg(format!("{}", self.pid))
                .args(args)
                .status()?
                .code()
                .ok_or(util::AnnotatedError::new("newuidmap errors"))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::net::TcpStream;

    struct TestHooks(RefCell<TcpStream>);

    impl TestHooks {
        fn at(&self, pos: &str) {
            self.0
                .borrow_mut()
                .write_all(pos.as_bytes())
                .expect("log socket write");
        }
    }

    impl ContainerHooks for TestHooks {
        fn at_start(&self) -> Result<(), Error> {
            self.at("A");
            Ok(())
        }
        fn unshare(&self) -> Result<(), Error> {
            self.at("B");
            Ok(())
        }
        fn set_id_map(&self, _pid: &Proc) -> Result<(), Error> {
            self.at("C");
            Ok(())
        }
        fn setup_priv(&self) -> Result<(), Error> {
            self.at("D");
            Ok(())
        }
        fn setup(&self) -> Result<(), Error> {
            self.at("E");
            Ok(())
        }
    }

    #[test]
    fn lifecycle() {
        let (mut me, dut) = util::socketpair().expect("socketpair");

        runc(&TestHooks(RefCell::new(dut))).expect("runc");
        me.set_nonblocking(true).unwrap();

        let mut result = String::new();
        me.read_to_string(&mut result).expect("Read results");
        assert_eq!(result, "ABCDE");
    }

    #[test]
    fn map_args() {
        let actual = IdMap::new_uid(0).add(0, 1, 2).add(15, 16, 2).map_args();

        assert_eq!(actual, &["0", "1", "2", "15", "16", "2"]);
    }

    #[test]
    fn map_file() {
        let actual = IdMap::new_uid(0).add(0, 1, 2).add(15, 16, 2).map_file();

        assert_eq!(actual, "0 1 2\n15 16 2\n");
    }
}
