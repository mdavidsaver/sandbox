use std::error::Error;
use std::process::exit;
use std::{net, fs, env};
use std::io::{self, Write, Read};
use std::path::Path;

use libc;

use fork::Fork;

use sandbox;
use sandbox::Annotatable;

struct Args {
    cmd: String,
    args: Vec<String>,
}

fn write_file(name: &str, buf: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new().write(true).open(Path::new(name))?;
    file.write_all(buf)
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

fn handle_child(mut toparent: net::TcpStream, args: Args) -> Result<(), Box<dyn Error>> {

    sandbox::unshare(libc::CLONE_NEWNS|libc::CLONE_NEWPID|libc::CLONE_NEWUSER|libc::CLONE_NEWCGROUP)
    .map_err(|err| { err.annotate("unshare") })?;
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
        handle_grandchild(args).expect("grandchild error");
        exit(0);
    }
    Err(e) => {
        eprintln!("Unable to fork grandchild {}", e);
        exit(1);
    }
    }
}

fn handle_grandchild(args: Args) -> Result<(), Box<dyn Error>>  {
    // we are PID 1 with full capabilities

    let cwd = env::current_dir()?;

    let noopt = libc::MS_NODEV|libc::MS_NOEXEC|libc::MS_NOSUID|libc::MS_RELATIME;

    sandbox::mount(&"", &"/", &"", libc::MS_REC|libc::MS_SLAVE, None)?;

    fs::create_dir_all(Path::new("/proc"))?;
    sandbox::mount(&"none", &"/proc", &"proc", noopt, None)?;

    //fs::create_dir_all(Path::new("/sys/fs/cgroup/unified"))?;
    //sandbox::mount(&"none", &"/sys/fs/cgroup/unified", &"cgroup2", noopt, None)?;

    fs::create_dir_all(Path::new("/tmp"))?;
    sandbox::mount(&"none", &"/tmp", &"tmpfs", noopt, None)?;

    sandbox::Exec::new(args.cmd)?
                    .args(args.args)?
                    .exec()?;

    Ok(()) // never reached
}

fn main() {
    let rawargs = env::args().collect::<Vec<String>>();
    if rawargs.len()<=1 {
        eprintln!("Usage: {} <cmd> [args ...]", rawargs[0]);
        exit(1);
    }

    let args = Args {
        cmd: rawargs[1].clone(),
        args: rawargs[2..].to_vec(),
    };

    let (parent, child) = sandbox::socketpair().expect("socketpair");

    match fork::fork() {
    Ok(Fork::Parent(pid)) => {
        drop(child);
        handle_parent(pid, parent).expect("parent error");
        exit(0);
    }
    Ok(Fork::Child) => {
        drop(parent);
        handle_child(child, args).expect("child error");
        exit(0);
    }
    Err(e) => {
        eprintln!("Unable to fork child {}", e);
        exit(1);
    }
    }
}
