//! Linux container (aka. namespace) management.
//!
//! Handles the double `fork()` needed to place a process into newly created namespaces.
use std::collections::BTreeMap;
use std::error;
use std::io::{self, Read, Write};
use std::net;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::process::{exit, Command};

use log::debug;

use libc;

use super::proc::fork;
use super::{err, ext, util};

pub use super::proc::Proc;

pub type Error = Box<dyn error::Error + 'static>;
pub type Result<T> = std::result::Result<T, Error>;

/// Container lifecycle hooks
///
/// Methods called via. `runc()`
///
/// ```text
/// runc() \  # in parent process
///        |- ContainerHooks::at_start()
///        |- fork() # create child process
///        |  \- ContainerHooks::unshare()
///        |-- | - ContainerHooks::set_id_map()
///        |   |-- fork() # create grandchild process
///        |   |   \- ContainerHooks::setup_priv()
///        |   |    |- Drop privilege
///        |   |    |- ContainerHooks::setup()
///        |   |    \- execvpe()
///        |   \- waitpid() # child waits for grandchild
///        \- waitpid() # parent waits for child
/// ```
#[allow(unused_variables)]
pub trait ContainerHooks {
    /// Called in parent process before child is forked
    fn at_start(&self) -> Result<()> {
        Ok(())
    }
    /// Called from child process when time to unshare()
    fn unshare(&self) -> Result<()> {
        Ok(())
    }
    /// Called from parent when time to set child uid/gid_map.
    fn set_id_map(&self, pid: &Proc) -> Result<()> {
        Ok(())
    }
    /// Called from grandchild with full privilege (all capabilities)
    fn setup_priv(&self) -> Result<()> {
        Ok(())
    }
    /// Called from grandchild with final privilege (no capabilities)
    fn setup(&self) -> Result<()> {
        Ok(())
    }
}

fn handle_parent<H: ContainerHooks>(
    hooks: &H,
    mut pid: Proc,
    mut tochild: net::TcpStream,
) -> Result<i32> {
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
    util::Cap::current()?.clear().update()?;
    // wait for child to exit
    Ok(pid.park()?)
}

fn handle_child<H: ContainerHooks>(hooks: &H, toparent: RawFd) -> Result<()> {
    let mut toparent = unsafe { net::TcpStream::from_raw_fd(toparent) };
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
                eprintln!("       or enable non-privileged user namespaces by eg.");
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
    debug!(
        "Child Perms uid {},{} gid {},{}",
        util::getuid(),
        util::geteuid(),
        util::getgid(),
        util::getegid()
    );
    debug!("Cap {}", util::Cap::current()?);

    let mut pid = fork(|| handle_grandchild(hooks))?;

    debug!("Forked Grandchild {}", pid);
    debug!("Child park");
    // drop SUID-ness
    util::setegid(util::getgid())?;
    util::seteuid(util::getuid())?;
    util::Cap::current()?.clear().update()?;
    // wait for child to exit
    exit(pid.park()?);
}

fn handle_grandchild<H: ContainerHooks>(hooks: &H) -> Result<()> {
    debug!("Grandchild");

    debug!(
        "Initial Perms uid {},{} gid {},{}",
        util::getuid(),
        util::geteuid(),
        util::getgid(),
        util::getegid()
    );
    debug!("Cap {}", util::Cap::current()?);

    // clear SUID-ness
    util::setegid(util::getgid())?;
    util::seteuid(util::getuid())?;
    // effective capabilities have been cleared.  permitted remain unset
    // re-activate all capabilities
    util::Cap::current()?.activate().update()?;

    debug!(
        "Update Perms uid {},{} gid {},{}",
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
    debug!(
        "Final Perms uid {},{} gid {},{}",
        util::getuid(),
        util::geteuid(),
        util::getgid(),
        util::getegid()
    );
    debug!("Cap {}", util::Cap::current()?);

    hooks.setup()?;
    Ok(())
}

/// Launch container with given hooks.  Blocks until container process 1 exits.
/// Returns with container process 1 exit code.
/// May be interrupted by `SIGINT`.
pub fn runc<H: ContainerHooks>(hooks: &H) -> Result<i32> {
    // communications between parent and child to coordinate SetIdMap()

    hooks.at_start()?;
    //.annotate("HOOK at_start()")?;

    let (parent, child) = util::socketpair()?;
    let child_fd = child.as_raw_fd();

    let pid = fork(|| handle_child(hooks, child_fd))?;

    drop(child);
    debug!("Forked Child {}", pid);
    handle_parent(hooks, pid, parent)
}

/// Helper for setting up UID and GID mappings for a new user namespace.
///
/// Acts either by directly manipulating `/proc/<pid>/uid_map` and `/proc/<pid>/gid_map`,
/// or calling out to the `newuidmap` and `newgidmap` executables when necessary
/// (unprivileged user namespace).
pub struct IdMap {
    pid: libc::pid_t,
    isuid: bool,
    map: BTreeMap<u32, (u32, u32)>,
}

impl IdMap {
    /// Start a new UID mapping
    pub fn new_uid(pid: libc::pid_t) -> IdMap {
        IdMap {
            pid,
            isuid: true,
            map: BTreeMap::new(),
        }
    }

    /// Start a new GID mapping
    pub fn new_gid(pid: libc::pid_t) -> IdMap {
        IdMap {
            pid,
            isuid: false,
            map: BTreeMap::new(),
        }
    }

    /// Add a mapping of `[start, start+count)` in the parent namespace
    /// to `[end, end+count)` in the new child namespace.
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

    /// Print the mapping in the format used by `/proc/<pid>/uid_map` and `/proc/<pid>/gid_map`
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

    /// Apply the mapping to the target process.
    pub fn write(&self) -> Result<()> {
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
                .ok_or(err::Error::UIDMap)?;
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
        fn at_start(&self) -> Result<()> {
            self.at("A");
            Ok(())
        }
        fn unshare(&self) -> Result<()> {
            self.at("B");
            Ok(())
        }
        fn set_id_map(&self, _pid: &Proc) -> Result<()> {
            self.at("C");
            Ok(())
        }
        fn setup_priv(&self) -> Result<()> {
            self.at("D");
            Ok(())
        }
        fn setup(&self) -> Result<()> {
            self.at("E");
            Ok(())
        }
    }

    #[test]
    fn lifecycle() {
        let (mut me, dut) = util::socketpair().expect("socketpair");

        runc(&TestHooks(RefCell::new(dut))).expect("runc");
        //me.set_nonblocking(true).unwrap();

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
