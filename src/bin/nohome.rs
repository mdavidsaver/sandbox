use std::error::Error;
use std::process::exit;
use std::{net, fs, env};
use std::io::{Write, Read};
use std::path::Path;

use libc;

use fork::Fork;

use sandbox;
use sandbox::AnnotateResult;

fn write_file<P: AsRef<Path>>(name: P, buf: &[u8]) -> Result<(), sandbox::AnnotatedError> {
    let mut file = fs::OpenOptions::new().write(true).open(name.as_ref())
        .annotate(format!("write_file {} open",name.as_ref().display()))?;
    file.write_all(buf)
        .annotate(format!("write_file {} I/O",name.as_ref().display()))
}

fn mkdirs<S: AsRef<Path>>(name: S) -> Result<(), sandbox::AnnotatedError> {
    fs::create_dir_all(name.as_ref())
        .annotate(format!("mkdirs {}", name.as_ref().display()))
}

fn handle_parent(pid: libc::pid_t, mut tochild: net::TcpStream) -> Result<(), Box<dyn Error>> {

    // wait for child to unshare()
    let mut msg = vec![0; 1];
    tochild.read_exact(&mut msg)?;

    write_file(format!("/proc/{}/uid_map", pid), "0          0 4294967295\n".as_bytes())?;
    write_file(format!("/proc/{}/gid_map", pid), "0          0 4294967295\n".as_bytes())?;

    // notify client to proceed
    tochild.write_all(".".as_bytes())?;

    // wait for child to exit
    exit(sandbox::park(pid)?);
}

fn handle_child(mut toparent: net::TcpStream) -> Result<(), Box<dyn Error>> {

    sandbox::unshare(libc::CLONE_NEWNS|libc::CLONE_NEWPID|libc::CLONE_NEWUSER|libc::CLONE_NEWCGROUP)
        .annotate("unshare")?;
    // This process is now in the new mount, user, and cgroup namespaces.
    // But remains in the original pid namespace.
    // The grandchild will be pid 1 in the new pid namespace.

    // ask parent to setup uid/gid maps
    toparent.write_all(".".as_bytes())?;

    // wait for parent
    let mut msg = vec![0; 1];
    toparent.read_exact(&mut msg)?;

    match fork::fork() {
    Ok(Fork::Parent(pid)) => {
        exit(sandbox::park(pid)?);
    }
    Ok(Fork::Child) => {
        handle_grandchild().expect("grandchild error");
        exit(0);
    }
    Err(e) => {
        eprintln!("Unable to fork grandchild {}", e);
        exit(1);
    }
    }
}

fn handle_grandchild() -> Result<(), Box<dyn Error>>  {
    // we are PID 1 with full capabilities

    let tmp = Path::new("/tmp");

    // Taking notion of /home from caller's environment.
    // Not validated.  Should be ok as we will only hide,
    // and never grant more visibility or permission.
    let home = Path::new(&env::var("HOME")?).canonicalize()?;
    if !home.is_absolute() {
        eprintln!("$HOME must be an absolute path");
        exit(1);
    }

    let cwd = env::current_dir()?.canonicalize()?;
    let relwd = cwd.strip_prefix(&home)
        .annotate(format!("Run under {}, not {}", home.display(), cwd.display()))?;

    // temp location of cwd under /tmp
    let twd = tmp.join(relwd);

    let noopt = libc::MS_NODEV|libc::MS_NOEXEC|libc::MS_NOSUID|libc::MS_RELATIME;

    sandbox::mount("", "/", "", libc::MS_REC|libc::MS_SLAVE)?;

    mkdirs("/proc")?;
    sandbox::mount("none", "/proc", "proc", noopt)?;

    mkdirs("/sys/fs/cgroup")?;
    sandbox::mount("none", "/sys/fs/cgroup", "tmpfs", noopt)?;

    mkdirs("/sys/fs/cgroup/unified")?;
    sandbox::mount("none", "/sys/fs/cgroup/unified", "cgroup2", noopt)?;

    // begin preparing replacement /home
    // will move /tmp -> /home after binding
    sandbox::mount("none", &tmp, "tmpfs", noopt)?;

    // bind $CWD under new $HOME
    mkdirs(&twd)?;
    sandbox::mount(&cwd, &twd, "", libc::MS_BIND)?;

    // hide real /home
    sandbox::mount(&tmp, &home, "", libc::MS_MOVE)?;

    // hide real temporary files to prevent snooping
    sandbox::mount("none", "/tmp", "tmpfs", noopt)?;
    mkdirs("/var/tmp")?;
    sandbox::mount("none", "/var/tmp", "tmpfs", noopt)?;

    // switch to new FS tree.  (avoid ../ escape)
    env::set_current_dir(cwd)?;

    // drop perm
    sandbox::setegid(sandbox::getgid())?;
    sandbox::seteuid(sandbox::getuid())?;
    sandbox::capclear()?;
    eprintln!("Cap {:?}", sandbox::capget(0)?);

    let rawargs = env::args().collect::<Vec<String>>();
    if rawargs.len()<=1 {
        eprintln!("Usage: {} <cmd> [args ...]", rawargs[0]);
        exit(1);
    }

    sandbox::Exec::new(&rawargs[1])?
                    .args(&rawargs[1..].to_vec())?
                    .exec()?;

    Ok(()) // never reached
}

fn main() -> Result<(), Box<dyn Error>> {

    let (parent, child) = sandbox::socketpair()?;

    match fork::fork() {
    Ok(Fork::Parent(pid)) => {
        drop(child);
        handle_parent(pid, parent)?;
        exit(0);
    }
    Ok(Fork::Child) => {
        drop(parent);
        handle_child(child)?;
        exit(0);
    }
    Err(e) => {
        eprintln!("Unable to fork child {}", e);
        exit(1);
    }
    }
}
