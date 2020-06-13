use std::error::Error;
use std::process::exit;
use std::{net, fs, env};
use std::io::{self, Write, Read};
use std::path::Path;

use libc;

use fork::Fork;

use sandbox;
use sandbox::AnnotateResult;

fn write_file(name: &str, buf: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new().write(true).open(Path::new(name))?;
    file.write_all(buf)
}

fn mkdirs<S: AsRef<str>>(name: S) -> Result<(), sandbox::AnnotatedError> {
    fs::create_dir_all(Path::new(name.as_ref()))
        .annotate(format!("mkdirs {}", name.as_ref()))
}

fn handle_parent(pid: libc::pid_t, mut tochild: net::TcpStream) -> Result<(), Box<dyn Error>> {

    // wait for child to unshare()
    let mut msg = vec![0; 1];
    tochild.read_exact(&mut msg)?;

    write_file(&format!("/proc/{}/uid_map", pid), "0          0 4294967295\n".as_bytes())?;
    write_file(&format!("/proc/{}/gid_map", pid), "0          0 4294967295\n".as_bytes())?;

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

    let noopt = libc::MS_NODEV|libc::MS_NOEXEC|libc::MS_NOSUID|libc::MS_RELATIME;

    sandbox::mount(&"", &"/", &"", libc::MS_REC|libc::MS_SLAVE, None)?;

    mkdirs("/proc")?;
    sandbox::mount(&"none", &"/proc", &"proc", noopt, None)?;

    mkdirs("/sys/fs/cgroup")?;
    sandbox::mount(&"none", &"/sys/fs/cgroup", &"tmpfs", noopt, None)?;

    mkdirs("/sys/fs/cgroup/unified")?;
    sandbox::mount(&"none", &"/sys/fs/cgroup/unified", &"cgroup2", noopt, None)?;

    mkdirs("/tmp")?;
    sandbox::mount(&"none", &"/tmp", &"tmpfs", noopt, None)?;

    mkdirs("/var/tmp")?;
    sandbox::mount(&"none", &"/var/tmp", &"tmpfs", noopt, None)?;

    // drop perm

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

    let _cwd = env::current_dir()?;

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
