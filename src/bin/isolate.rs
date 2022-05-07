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
    allownet: bool,
    args: Vec<String>,
    tdir: &'a Path,
    writable: Vec<PathBuf>,
    cwd: PathBuf,
}

impl<'a> ContainerHooks for Isolate<'a> {
    fn unshare(&self) -> Result<(), Error> {
        log::debug!("child unshare()");
        let mut flags =
            libc::CLONE_NEWNS | libc::CLONE_NEWPID | libc::CLONE_NEWCGROUP | libc::CLONE_NEWIPC;
        if !self.allownet {
            flags |= libc::CLONE_NEWNET;
        }
        if self.isuser {
            flags |= libc::CLONE_NEWUSER;
        }
        util::unshare(flags)?;
        Ok(())
    }

    fn set_id_map(&self, pid: &Proc) -> Result<(), Error> {
        log::debug!("Setup ID mapping");
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
        log::debug!("Privlaged setup");

        if !self.allownet {
            net::configure_lo()?;
        }

        // begin by isolating our new mount ns
        util::mount("", "/", "", libc::MS_REC | libc::MS_PRIVATE)?;

        // make /proc for our new PID namespace available early
        util::mount("proc", "/proc", "proc", NOOPT)?;

        let new_root = util::mkdir(path!(self.tdir, "root"))?;
        let new_tmp = path!(&new_root, "tmp");
        let new_proc = path!(&new_root, "proc");
        let new_devshm = path!(&new_root, "dev", "shm");

        log::debug!("Prepare new root at {}", new_root.display());

        // mount --rbind / /tmp/.../root
        util::mount("/", &new_root, "", libc::MS_BIND | libc::MS_REC)?;

        //util::Exec::new("bash")?.exec()?;

        // disconnect some FS we definately won't use (if they are mount points)
        util::umount_lazy(&new_proc)?;
        util::maybe_umount_lazy(&new_devshm)?;
        util::maybe_umount_lazy(&new_tmp)?;
        util::maybe_umount_lazy(path!(&new_root, "var", "tmp"))?;

        log::debug!("Fixup non-root mounts");

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

        log::debug!("Add special mounts");

        util::mount("none", &new_proc, "proc", NOOPT)?;
        util::mount("none", &new_tmp, "tmpfs", NOOPT)?;
        util::mount("none", &new_devshm, "tmpfs", NOOPT)?;
        util::mount("none", path!(&new_root, "var", "tmp"), "tmpfs", NOOPT)?;

        // bind writable
        for wdir in &self.writable {
            let tdir = path!(&new_root, wdir.strip_prefix("/").unwrap());
            log::debug!("Make RW: {}", wdir.display());

            if tdir.exists() {
                // nothing to do
            } else if tdir.starts_with("/tmp") {
                util::clonedirs(&wdir, &new_root)?;
            } else {
                log::error!("PWD in unallowed location");
            }

            util::mount(&wdir, tdir, "", libc::MS_BIND)?;
        }

        log::debug!("Switch to new root");

        util::mkdir(path!(&new_tmp, "oldroot"))?;

        util::umount_lazy("/proc")?; // mounted above, no longer needed

        env::set_current_dir(&new_root)?;
        util::pivot_root(".", "tmp/oldroot")?;

        env::set_current_dir("/")?;

        util::umount_lazy("/tmp/oldroot")?;
        util::rmdir("/tmp/oldroot")?;

        log::debug!("Switched to new root");

        Ok(())
    }

    fn setup(&self) -> Result<(), Error> {
        env::set_current_dir(&self.cwd)?;

        log::debug!("EXEC {:?}", &self.args[0..]);
        env::set_var("VIRTUAL_ENV", "isolated");

        util::Exec::new(&self.args[0])?
            .args(&self.args[0..])?
            .exec()?;

        Ok(())
    }
}

fn usage() {
    let execname = env::args().next().unwrap();
    eprint!("Usage: {execname} [-h] [-n|--net] [-W|--rw <dir>] <cmd> [args ...]

Execute command in an isolated environment.  By default only $PWD
will be writable, with no network access allowed.

Options:
    -h             - Show this message
    -n --net       - Allow network access
    -W --rw <dir>  - Allow writes to part of the directory tree

eg. prevent a build from accidentally changing files outside of the build directory.
  $ isolate make

");
}

fn main() -> Result<(), Error> {
    env_logger::init();
    let cwd = env::current_dir()?.canonicalize()?;
    if !cwd.is_absolute() {
        eprintln!("curdir is not absolute?!?");
        process::exit(2);
    }

    let mut rawargs = env::args().skip(1).collect::<Vec<String>>();
    let mut allownet = false;
    let mut writable = vec![cwd.clone()];
    let mut add_writable = |path: &PathBuf| -> Result<(), Error> {
        writable.push((&cwd).join(path).canonicalize()?);
        Ok(())
    };

    while !rawargs.is_empty() {
        if !rawargs[0].starts_with("-") {
            break;
        } else if rawargs[0] == "-n" || rawargs[0] == "--net" {
            allownet = true;
        } else if rawargs[0] == "-W" || rawargs[0] == "--rw" {
            add_writable(&PathBuf::from(
                rawargs.get(1).expect("-W/--rw expects argument"),
            ))?;
            rawargs.remove(0);
        } else {
            usage();
            if rawargs[0] == "-h" {
                process::exit(0);
            } else {
                eprintln!("Unknown argument: {}", rawargs[0]);
                process::exit(1);
            }
        }
        rawargs.remove(0);
    }
    if rawargs.len() == 0 {
        usage();
        process::exit(1);
    }

    let tdir = TempDir::new().unwrap();
    util::chown(tdir.path(), util::getuid(), util::getgid())?;

    let cont = Isolate {
        isuser: !util::Cap::current()?.effective(util::CAP_SYS_ADMIN),
        allownet: allownet,
        args: rawargs,
        tdir: tdir.path(),
        writable: writable,
        cwd: env::current_dir()?,
    };

    let ret = runc(&cont);
    drop(tdir);
    process::exit(ret?);
}
