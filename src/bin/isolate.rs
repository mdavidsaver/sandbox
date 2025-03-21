use std::collections::HashSet;
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
const TMPOPT: libc::c_ulong = libc::MS_NODEV | libc::MS_NOSUID | libc::MS_RELATIME;

#[derive(Debug)]
enum MountType {
    ReadOnly,
    Writable,
    Tmp,
}

struct Isolate<'a> {
    isuser: bool,
    allownet: bool,
    args: Vec<String>,
    tdir: &'a Path,
    mounts: Vec<(MountType, PathBuf)>,
    cwd: PathBuf,
    bridge: std::cell::Cell<Option<net::Bridge>>,
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
            self.bridge.set(Some(net::dummy_bridge()?));
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
        util::mount("none", &new_tmp, "tmpfs", TMPOPT)?;
        util::mount("none", &new_devshm, "tmpfs", NOOPT)?;
        util::mount("none", path!(&new_root, "var", "tmp"), "tmpfs", TMPOPT)?;

        // user binds
        // first do any RO/W binds
        for (mtype, dir) in &self.mounts {
            let tdir = path!(&new_root, dir.strip_prefix("/")?);
            log::debug!("Bind as {mtype:?}: {}", dir.display());

            match mtype {
                MountType::ReadOnly => {
                    // creating a RO bind mount is a two step process.
                    // first create a normal bind mount (rw vs. ro depends on parent mount)
                    util::mount(&dir, &tdir, "", libc::MS_BIND)?;

                    // now do a re-mount as RO.
                    // must look up mount info each time.
                    let opts = Mounts::current()?.lookup(&tdir)?.options;

                    util::mount(
                        "",
                        &tdir,
                        "",
                        opts | libc::MS_REMOUNT | libc::MS_RDONLY | libc::MS_BIND,
                    )?;
                }
                MountType::Writable => {
                    if tdir.exists() {
                        // nothing to do
                    } else if tdir.starts_with("/tmp") {
                        util::clonedirs(&dir, &new_root)?;
                    } else {
                        log::error!("PWD in unallowed location");
                    }

                    util::mount(&dir, tdir, "", libc::MS_BIND)?;
                }
                MountType::Tmp => {} // handle below
            }
        }
        // now overlay with any tmpfs binds
        for (mtype, dir) in &self.mounts {
            let tdir = path!(&new_root, dir.strip_prefix("/")?);
            match mtype {
                MountType::Tmp => {
                    util::mount("", &tdir, "tmpfs", libc::MS_NODEV | libc::MS_NOSUID)?;
                }
                _ => {}
            }
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
        log::debug!("chdir({:?})", self.cwd);
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
    eprint!(
        "Usage: {execname} [-h] [-N|--net] [-W|--rw <dir>] [-O|--ro <dir>] <cmd> [args ...]

Execute command in an isolated environment.  By default only $PWD
will be writable, with no network access allowed.

Options:
    -h --help      - Show this message
    -N --net       - Allow network access
    -C --chdir     - Switch $PWD
    -c --no-pwd    - Deny writes to $PWD  (shorthand for \"-O .\")
    -W --rw <dir>  - Allow writes to part of the directory tree
    -O --ro <dir>  - Deny writes to part of the directory tree
    -T --tmp <dir> - Bind empty tmpfs to a directory

eg. prevent a build from accidentally changing files outside of the build directory.
  $ isolate make

"
    );
}

fn main() -> Result<(), Error> {
    sandbox::logging::setup().unwrap();

    let mut cwd = env::current_dir()?.canonicalize()?;
    if !cwd.is_absolute() {
        eprintln!("curdir is not absolute?!?");
        process::exit(2);
    }

    let mut iargs = env::args().skip(1).peekable();
    let mut allownet = false;
    let mut mounts = vec![];

    // order first, so the any subsequent -O ./whatever take precedence
    // overwritten below after processing -C
    mounts.push((MountType::Writable, cwd.clone()));

    while let Some(arg) = iargs.peek() {
        if !arg.starts_with("-") {
            break;
        }
        let arg = iargs.next().unwrap();
        let mut argval = || iargs.next().expect(&format!("{arg} expects argument"));

        if arg == "-n" || arg == "-N" || arg == "--net" {
            allownet = true;
        } else if arg == "-c" || arg == "--no-pwd" {
            mounts.push((MountType::ReadOnly, cwd.clone()));
        } else if arg == "-W"
            || arg == "--rw"
            || arg == "-O"
            || arg == "--ro"
            || arg == "-T"
            || arg == "--tmp"
        {
            let mtype = if arg == "-O" || arg == "--ro" {
                MountType::ReadOnly
            } else if arg == "-W" || arg == "--rw" {
                MountType::Writable
            } else if arg == "-T" || arg == "--tmp" {
                MountType::Tmp
            } else {
                unreachable!();
            };

            let dir: PathBuf = argval().into();
            if dir.is_dir() {
                mounts.push((mtype, dir.canonicalize()?));
            } else {
                log::warn!("Ignore non-existant directory: {arg} {}", dir.display());
            }
        } else if arg == "-C" || arg == "--chdir" {
            let dir: PathBuf = argval().into();
            cwd = dir.canonicalize()?;
        } else if arg == "-h" || arg == "--help" {
            usage();
            return Ok(());
        } else {
            usage();
            eprintln!("Unknown argument: {arg}");
            process::exit(1);
        }
    }

    mounts[0] = (MountType::Writable, cwd.clone());

    // remove duplicates in favor of last
    let mounts = {
        let mut mseen = HashSet::new();
        let mut munique = vec![];
        for (mtype, dir) in mounts.into_iter() {
            if mseen.contains(&dir) {
                munique = munique.into_iter().filter(|(_, d)| d != &dir).collect();
            }
            mseen.insert(dir.clone());
            munique.push((mtype, dir));
        }
        munique
    };

    let rawargs = iargs.collect::<Vec<String>>();

    if rawargs.len() == 0 {
        usage();
        process::exit(1);
    }

    let tdir = TempDir::new()?;
    util::chown(tdir.path(), util::getuid(), util::getgid())?;

    let cont = Isolate {
        isuser: !util::Cap::current()?.effective(util::CAP_SYS_ADMIN),
        allownet,
        args: rawargs,
        tdir: tdir.path(),
        mounts,
        cwd,
        bridge: std::cell::Cell::new(None),
    };

    let ret = runc(&cont);
    drop(tdir);
    process::exit(ret?);
}
