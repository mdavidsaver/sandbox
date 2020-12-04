use std::path::{Path, PathBuf};
use std::{env, process};

use log;

use sandbox::container::{ContainerHooks, IdMap, Proc};
use sandbox::fs::Mounts;
use sandbox::path;
use sandbox::tempdir::TempDir;
use sandbox::{net, util};
use sandbox::{runc, Error};

const NOOPT: libc::c_ulong = libc::MS_NODEV | libc::MS_NOEXEC | libc::MS_NOSUID | libc::MS_RELATIME;

struct Isolate<'a> {
    isuser: bool,
    args: Vec<String>,
    tdir: &'a Path,
    cwd: PathBuf,
}

impl<'a> Isolate<'a> {
    pub fn new<I>(tdir: &'a Path, args: I) -> Result<Self, Error>
    where
        I: IntoIterator,
        I::Item: Into<String>,
    {
        Ok(Self {
            isuser: !util::Cap::current()?.effective(util::CAP_SYS_ADMIN),
            args: args.into_iter().map(|e| e.into()).collect(),
            tdir: tdir,
            cwd: env::current_dir()?,
        })
    }
}

impl<'a> ContainerHooks for Isolate<'a> {
    fn unshare(&self) -> Result<(), Error> {
        log::debug!("child unshare()");
        let mut flags = libc::CLONE_NEWNS
            | libc::CLONE_NEWPID
            | libc::CLONE_NEWCGROUP
            | libc::CLONE_NEWIPC
            | libc::CLONE_NEWNET;
        if self.isuser {
            flags |= libc::CLONE_NEWUSER;
        }
        util::unshare(flags)?;
        Ok(())
    }

    fn set_id_map(&self, pid: &Proc) -> Result<(), Error> {
        // Setup 1-1 mapping
        if self.isuser {
            log::debug!("Setup 1-1 UID mapping");
            let uid = util::getuid();
            let gid = util::getgid();
            IdMap::new_uid(pid.id()).add(uid, uid, 1).write()?;
            IdMap::new_gid(pid.id()).add(gid, gid, 1).write()?;
        }
        Ok(())
    }

    fn setup_priv(&self) -> Result<(), Error> {
        net::configure_lo()?;

        // begin by isolating our new mount ns
        util::mount("", "/", "", libc::MS_REC | libc::MS_PRIVATE)?;

        // make /proc for our new PID namespace available early
        util::mount("proc", "/proc", "proc", NOOPT)?;

        let new_root = util::mkdir(path!(self.tdir, "root"))?;
        let new_tmp = path!(&new_root, "tmp");
        let new_proc = path!(&new_root, "proc");
        let new_devshm = path!(&new_root, "dev", "shm");

        // mount --rbind / /tmp/.../root
        util::mount("/", &new_root, "", libc::MS_BIND | libc::MS_REC)?;

        //util::Exec::new("bash")?.exec()?;

        // disconnect some FS we definately won't use (if they are mount points)
        util::umount_lazy(&new_proc)?;
        util::maybe_umount_lazy(&new_devshm)?;
        util::maybe_umount_lazy(&new_tmp)?;
        util::maybe_umount_lazy(path!(&new_root, "var", "tmp"))?;

        for mp in Mounts::current()?.into_iter() {
            if !mp.mount_point.starts_with(&new_root) {
                continue;
            }
            log::debug!("Visit: {}", &mp);

            // black-list some fs-types
            if !self.isuser && ["cgroup", "cgroup2", "debugfs"].contains(&mp.fstype.as_str()) {
                log::debug!("Unmount: {}", mp.mount_point.display());
                util::umount_lazy(&mp.mount_point)?;
            }

            if mp.has_option(libc::MS_RDONLY) {
                continue;
            }

            // try to remount phyisical and various tmpfs-like as read-only
            if mp.source.starts_with("/dev/") || ["tmpfs", "ramfs"].contains(&mp.fstype.as_str()) {
                log::debug!("Make RO: {}", mp.mount_point.display());
                match util::mount(
                    "",
                    &mp.mount_point,
                    "",
                    mp.options | libc::MS_REMOUNT | libc::MS_RDONLY | libc::MS_BIND,
                ) {
                    // this mount point may not be accessible to a non-privlaged user.  eg. under /root
                    Err(err)
                        if self.isuser && err.is_io_error(std::io::ErrorKind::PermissionDenied) =>
                    {
                        Ok(())
                    }
                    other => other,
                }?;
            }
        }

        util::mount("none", &new_proc, "proc", NOOPT)?;
        util::mount("none", &new_tmp, "tmpfs", NOOPT)?;
        util::mount("none", &new_devshm, "tmpfs", NOOPT)?;
        util::mount("none", path!(&new_root, "var", "tmp"), "tmpfs", NOOPT)?;

        // bind PWD R/W
        util::mount(
            &self.cwd,
            path!(&new_root, self.cwd.strip_prefix("/").unwrap()),
            "",
            libc::MS_BIND,
        )?;

        util::mkdir(path!(&new_tmp, "oldroot"))?;

        util::umount_lazy("/proc")?; // mounted above, no longer needed

        env::set_current_dir(&new_root)?;
        util::pivot_root(".", "tmp/oldroot")?;

        env::set_current_dir("/")?;

        util::umount_lazy("/tmp/oldroot")?;
        util::rmdir("/tmp/oldroot")?;

        Ok(())
    }

    fn setup(&self) -> Result<(), Error> {
        env::set_current_dir(&self.cwd)?;

        log::debug!("EXEC {:?}", &self.args[0..]);

        util::Exec::new(&self.args[0])?
            .args(&self.args[0..])?
            .exec()?;

        Ok(())
    }
}

fn main() -> Result<(), Error> {
    env_logger::init();

    let rawargs = env::args().collect::<Vec<String>>();
    if rawargs.len() <= 1 {
        eprintln!("Usage: {} <cmd> [args ...]", rawargs[0]);
        process::exit(1);
    }

    let tdir = TempDir::new().unwrap();
    util::chown(tdir.path(), util::getuid(), util::getgid())?;

    process::exit(runc(&Isolate::new(tdir.path(), &rawargs[1..])?)?);
}
