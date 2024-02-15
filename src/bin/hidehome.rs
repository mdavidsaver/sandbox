use std::path::Path;
use std::{env, process};

use libc;
use log::debug;

use sandbox::container::{ContainerHooks, IdMap, Proc};
use sandbox::util;
use sandbox::{runc, Error};

/// Container which executes a command with most of /home hidden
struct HideHome {
    isuser: bool,
    args: Vec<String>,
}

impl HideHome {
    pub fn new<I>(args: I) -> Result<HideHome, Error>
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        Ok(HideHome {
            isuser: !util::Cap::current()?.effective(util::CAP_SYS_ADMIN),
            args: args.into_iter().map(|e| e.into()).collect(),
        })
    }
}

impl ContainerHooks for HideHome {
    fn unshare(&self) -> Result<(), Error> {
        debug!("child unshare()");
        let mut flags = libc::CLONE_NEWNS | libc::CLONE_NEWPID | libc::CLONE_NEWCGROUP;
        if self.isuser {
            flags |= libc::CLONE_NEWUSER;
        }
        util::unshare(flags)?;
        Ok(())
    }

    fn set_id_map(&self, pid: &Proc) -> Result<(), Error> {
        // Setup 1-1 mapping
        if self.isuser {
            debug!("Setup 1-1 UID mapping");
            let uid = util::getuid();
            let gid = util::getgid();
            IdMap::new_uid(pid.id()).add(uid, uid, 1).write()?;
            IdMap::new_gid(pid.id()).add(gid, gid, 1).write()?;
        }
        Ok(())
    }

    fn setup_priv(&self) -> Result<(), Error> {
        let tmp = Path::new("/tmp");

        // Taking notion of /home from caller's environment.
        // Not validated.  Should be ok as we will only hide,
        // and never grant more visibility or permission.
        let home = Path::new(&env::var("HOME")?).canonicalize()?;
        if !home.is_absolute() {
            eprintln!("$HOME must be an absolute path");
            process::exit(1);
        }

        // The root of the tree we will hide.
        // parent of $HOME eg. /home
        // but not / itself (eg. $HOME==/root)
        let root = home
            .parent()
            .filter(|p| p != &Path::new("/"))
            .unwrap_or(&home);

        let cwd = env::current_dir()?.canonicalize()?;

        if cwd.starts_with(tmp) {
            eprintln!("Can't run under /tmp");
            process::exit(1);
        }

        let noopt = libc::MS_NODEV | libc::MS_NOEXEC | libc::MS_NOSUID | libc::MS_RELATIME;

        // begin by slaving the new mount ns
        util::mount("", "/", "", libc::MS_REC | libc::MS_SLAVE)?;

        // mount for the new PID ns
        util::mkdirs("/proc")?;
        util::mount("none", "/proc", "proc", noopt)?;

        // mount for the new cgroup ns
        util::mkdirs("/sys/fs/cgroup")?;
        util::mount("none", "/sys/fs/cgroup", "tmpfs", noopt)?;

        util::mkdirs("/sys/fs/cgroup/unified")?;
        util::mount("none", "/sys/fs/cgroup/unified", "cgroup2", noopt)?;

        // begin preparing replacement /home
        // will move after binding
        util::mount("none", &tmp, "tmpfs", noopt)?;

        if let Some(relwd) = cwd.strip_prefix(&root).ok() {
            // $CWD is under /home
            // temp locations of home and cwd under /tmp
            let twd = tmp.join(relwd);

            // bind $CWD under new $HOME
            util::mkdirs(&twd)?;
            util::mount(&cwd, &twd, "", libc::MS_BIND)?;
        } else {
            util::mkdirs(&home)?;
        }

        // hide real /home
        util::mount(&tmp, &root, "", libc::MS_MOVE)?;

        // hide real temporary files to prevent snooping
        util::mount("none", "/tmp", "tmpfs", noopt)?;
        util::mkdirs("/var/tmp")?;
        util::mount("none", "/var/tmp", "tmpfs", noopt)?;

        // switch to new FS tree.  (avoid ../ escape)
        env::set_current_dir(cwd)?;

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
    pretty_env_logger::init();

    let rawargs = env::args().collect::<Vec<String>>();
    if rawargs.len() <= 1 {
        eprintln!("Usage: {} <cmd> [args ...]", rawargs[0]);
        process::exit(1);
    }

    process::exit(runc(&HideHome::new(&rawargs[1..])?)?);
}
