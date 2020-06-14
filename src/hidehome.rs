use std::{env, process};
use std::path::Path;

use libc;
use log::debug;

use super::container::{ContainerHooks, Error, IdMap, Proc};
use super::util;
use super::util::AnnotateResult;

pub struct HideHome {
    args: Vec<String>,
}

impl HideHome {
    pub fn new<A, B, I> (cmd: A, args: I) -> HideHome
        where
            A: AsRef<str>,
            B: AsRef<str>,
            I: IntoIterator<Item=B>,
    {
        let mut cmd = vec![cmd.as_ref().to_string()];
        for arg in args {
            cmd.push(arg.as_ref().to_string());
        }
        HideHome {
            args: cmd,
        }
    }
}

impl ContainerHooks for HideHome {
    fn unshare(&self) -> Result<(), Error> {
        debug!("child unshare()");
        util::unshare(
            libc::CLONE_NEWNS | libc::CLONE_NEWPID | libc::CLONE_NEWUSER | libc::CLONE_NEWCGROUP,
        )?;
        Ok(())
    }

    fn set_id_map(&self, pid: &Proc) -> Result<(), Error> {
        // Setup 1-1 mapping
        IdMap::new_uid(pid.id())
        .add(0, 0, 0xffffffff)
        .write()?;
        IdMap::new_gid(pid.id())
        .add(0, 0, 0xffffffff)
        .write()?;
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

        // The root of the tree we ill hide.
        // parent of $HOME eg. /home
        // but not / itself (eg. $HOME==/root)
        let root = home
            .parent()
            .filter(|p| p != &Path::new("/"))
            .unwrap_or(&home);

        let cwd = env::current_dir()?.canonicalize()?;

        // enforce $PWD under $HOME
        cwd.strip_prefix(&home).annotate(format!(
            "Run under {}, not {}",
            home.display(),
            cwd.display()
        ))?;

        let relhome = home.strip_prefix(&root).annotate(format!(
            "Run under {}, not {}",
            root.display(),
            home.display()
        ))?;
        let relwd = cwd.strip_prefix(&root).annotate(format!(
            "Run under {}, not {}",
            root.display(),
            cwd.display()
        ))?;

        // temp locations of home and cwd under /tmp
        let _thome = tmp.join(relhome); // eg. /home/user -> /tmp/user
        let twd = tmp.join(relwd);

        let noopt = libc::MS_NODEV | libc::MS_NOEXEC | libc::MS_NOSUID | libc::MS_RELATIME;

        util::mount("", "/", "", libc::MS_REC | libc::MS_SLAVE)?;

        util::mkdirs("/proc")?;
        util::mount("none", "/proc", "proc", noopt)?;

        util::mkdirs("/sys/fs/cgroup")?;
        util::mount("none", "/sys/fs/cgroup", "tmpfs", noopt)?;

        util::mkdirs("/sys/fs/cgroup/unified")?;
        util::mount("none", "/sys/fs/cgroup/unified", "cgroup2", noopt)?;

        // begin preparing replacement /home
        // will move after binding
        util::mount("none", &tmp, "tmpfs", noopt)?;

        // bind $CWD under new $HOME
        util::mkdirs(&twd)?;
        util::mount(&cwd, &twd, "", libc::MS_BIND)?;

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
    
        debug!("EXEC {:?}", &self.args[1..]);

        util::Exec::new(&self.args[1])?
            .args(&self.args[1..])?
            .exec()?;

        Ok(())
    }
}
