//! Filesystem utilities...

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{fmt, fs};

use std::os::unix::fs::MetadataExt;

use log::{debug, warn};

use super::err::{Error, Result};

// like vec!() for a PathBuf
#[macro_export]
macro_rules! path {
    ($root:expr, $( $piece:expr ),*) => {
        {
            let mut temp = PathBuf::from($root);
            $(
                temp.push($piece);
            )*
            temp
        }
    }
}

/// Find the (parent) directory which is a mount point for this file/directory.
///
/// Returns either the provided `path` or a parent.
/// See src/find-mount-point.c in GNU coreutils
pub fn find_mount_point<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
    let path = path
        .as_ref()
        .canonicalize()
        .map_err(|e| Error::file("canonicalize", &path, e))?;
    let s = fs::metadata(&path).map_err(|e| Error::file("metadata", &path, e))?;

    //
    let mut dir = if s.file_type().is_dir() {
        &path
    } else {
        // canonicalize'd files always have a "parent"
        path.parent().unwrap()
    };

    loop {
        if let Some(next) = dir.parent() {
            let nexts = fs::metadata(&next).map_err(|e| Error::file("metadata", &next, e))?;
            // assume nexts.ftype==FileType::Dir
            if s.dev() != nexts.dev() || s.ino() == nexts.ino() {
                // parent is a different mount point
                return Ok(dir.to_path_buf());
            }
            dir = next;
        } else {
            // reached root, assumed to be a mountpoint
            return Ok(dir.to_path_buf());
        }
    }
}

/// cf. `Documentation/filesystems/proc.txt` in the Linux kernel source tree.
#[derive(Debug)]
pub struct MountInfo {
    pub id: u64,
    // parent
    // major:minor
    pub root: PathBuf,
    pub mount_point: PathBuf,
    pub options: u64,
    // optional fields
    pub fstype: String,
    pub source: String,
    // super options
    //pub
}

impl MountInfo {
    pub fn has_option(&self, opt: libc::c_ulong) -> bool {
        0 != (self.options & opt)
    }
}

impl fmt::Display for MountInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "mount={} fstype={} source={}",
            self.mount_point.display(),
            self.fstype,
            self.source
        )
    }
}

/// A list of file system mount points
#[derive(Debug)]
pub struct Mounts {
    points: HashMap<PathBuf, MountInfo>,
}

impl Mounts {
    /// Mount points in the namespace of the current process
    pub fn current() -> Result<Mounts> {
        Self::create(&"self")
    }

    /// Mount points in the namespace of the specified PID
    pub fn from_pid(pid: libc::pid_t) -> Result<Mounts> {
        Self::create(pid.to_string().as_str())
    }

    fn parse_line(line: &str) -> Result<MountInfo> {
        let mut liter = line.split_ascii_whitespace().peekable();

        // cf. Documentation/filesystems/proc.rst
        // lines like:
        // 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue
        // (0)(1)(2)   (3)   (4)      (5)      (6)   (7) (8)   (9)          (10)
        // where (6) may be repeated zero or more times.
        let id = liter.next().ok_or(Error::BadStr)?.parse::<_>()?;
        let _parent_id = liter.next().ok_or(Error::BadStr)?;
        let _dev = liter.next().ok_or(Error::BadStr)?;
        let root = liter.next().ok_or(Error::BadStr)?.into();
        let mount_point = liter.next().ok_or(Error::BadStr)?.into();
        let opts = liter.next().ok_or(Error::BadStr)?;
        loop {
            if let Some(next) = liter.peek() {
                if next == &"-" {
                    // end of option fields
                    break;
                }
                liter.next().unwrap();
            }
        }
        let sep = liter.next().ok_or(Error::BadStr)?;
        debug_assert_eq!(sep, "-");
        let fstype = liter.next().ok_or(Error::BadStr)?.into();
        let source = liter.next().ok_or(Error::BadStr)?.into();
        let _sopts = liter.next().ok_or(Error::BadStr)?;
        if liter.peek().is_some() {
            debug!("Ignoring extra mountinfo {:?}", line);
        }

        let mut options = 0;
        for opt in opts.split(',') {
            match opt {
                // cf. 'man 8 mount' and 'man 2 mount'
                "ro" => options |= libc::MS_RDONLY,
                "rw" => (),
                "noexec" => options |= libc::MS_NOEXEC,
                "nosuid" => options |= libc::MS_NOSUID,
                "nodev" => options |= libc::MS_NODEV,
                "noatime" => options |= libc::MS_NOATIME,
                "nodiratime" => options |= libc::MS_NODIRATIME,
                "relatime" => options |= libc::MS_RELATIME,
                "strictatime" => options |= libc::MS_STRICTATIME,
                _ => warn!("For {:?} ignore unknown option {:?}", opts, opt),
            }
        }

        Ok(MountInfo {
            id,
            // parent id
            // dev
            root,
            mount_point,
            options,
            // options fields
            fstype,
            source,
            // super options
        })
    }

    fn create(pid: &str) -> Result<Mounts> {
        let fname: PathBuf = [&"/proc", pid, "mountinfo"].iter().collect();

        let contents = fs::read_to_string(&fname).map_err(|e| Error::file("open", &fname, e))?;
        Self::parse(&contents, &fname)
    }

    fn parse(contents: &str, fname: &Path) -> Result<Mounts> {
        let lines: Vec<&str> = contents.lines().collect();

        // lines like:
        // 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue
        // (0)(1)(2)   (3)   (4)      (5)      (6)   (7) (8)   (9)          (10)
        // where (6) may be repeated zero or more times.

        // order is not certain.  '/' may not be first entry
        // so we pass ignore parents on first pass

        let mut infos = HashMap::new();

        for (lino, line) in lines.into_iter().enumerate() {
            let info = Self::parse_line(line).map_err(|_| {
                Error::parse(format!("Error parsing line {} : {:?}", lino, line), &fname)
            })?;
            let key = info.mount_point.clone();
            infos.insert(key, info);
        }

        if infos.is_empty() {
            Err(Error::MissingMount)?;
        }

        Ok(Mounts { points: infos })
    }

    /// Lookup the mount point for the provided path, which need not be a mount point.
    pub fn lookup<P: AsRef<Path>>(&self, path: P) -> Result<&MountInfo> {
        let mp = find_mount_point(path)?;
        self.points.get(&mp).ok_or_else(|| Error::MissingMount {})
    }
}

impl<'a> IntoIterator for &'a Mounts {
    type Item = &'a MountInfo;
    // oh for decltype()
    type IntoIter = std::collections::hash_map::Values<'a, PathBuf, MountInfo>;

    fn into_iter(self) -> Self::IntoIter {
        self.points.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cwd() {
        let cwd = std::env::current_dir().unwrap();
        let ret = find_mount_point(&cwd);
        ret.unwrap();
    }

    #[test]
    fn test_root() {
        let ret = find_mount_point(&"/").unwrap();
        assert_eq!(ret, Path::new(&"/"));
    }

    #[test]
    fn test_empty() {
        let ret = find_mount_point(&"");
        assert!(ret.is_err(), "{:?}", ret);
    }

    #[test]
    fn test_mountinfo_self() {
        let infos = Mounts::current().unwrap();
        let root = infos.lookup(&"/").unwrap();
        assert_eq!(root.mount_point.display().to_string(), "/");
    }

    #[test]
    fn test_mountinfo_static() {
        let inp = "
22 29 0:20 / /sys rw,nosuid,nodev,noexec,relatime shared:7 - sysfs sysfs rw
29 1 253:1 / / rw,noatime shared:1 - ext4 /dev/mapper/local-root rw,errors=remount-ro
"
        .trim_start();
        let infos = Mounts::parse(inp, &PathBuf::from(&"static")).unwrap();
        assert_eq!("sysfs", infos.lookup(&"/sys").unwrap().fstype);
        assert_eq!("ext4", infos.lookup(&"/").unwrap().fstype);
    }
}
