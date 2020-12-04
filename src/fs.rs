use std::{fmt, fs, rc};
use std::path::{Path, PathBuf};
use std::collections::HashMap;

use std::os::unix::fs::MetadataExt;

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

// cf. Documentation/filesystems/proc.txt
#[derive(Debug)]
pub struct MountInfo {
    pub id: u64,
    pub parent: Option<rc::Rc<MountInfo>>,
    // major:minor
    pub root: PathBuf,
    pub mount_point: PathBuf,
    pub options: Vec<String>,
    // optional fields
    pub fstype: String,
    pub source: String,
    // super options
    //pub 
}

impl MountInfo {
    pub fn is_root(&self) -> bool {
        return self.mount_point==Path::new("/") && self.parent.is_none();
    }

    pub fn has_option<S: AsRef<str>>(&self, opt: S) -> bool {
        for has in &self.options {
            if has==opt.as_ref() {
                return true;
            }
        }
        return false;
    }
}

impl fmt::Display for MountInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mount={} parent={} fstype={} source={}",
            self.mount_point.display(),
            self.parent.as_ref().map_or(&self.mount_point.clone(), |p| &p.mount_point).display(),
            self.fstype,
            self.source)
    }
}

#[derive(Debug)]
pub struct Mounts {
    points: HashMap<PathBuf, rc::Rc<MountInfo>>,
}

impl Mounts {
    pub fn current() -> Result<Mounts> {
        Self::create(&"self")
    }

    pub fn from_pid(pid: libc::pid_t) -> Result<Mounts> {
        Self::create(pid.to_string())
    }

    fn create<S: AsRef<str>>(pid: S) -> Result<Mounts> {
        
        let mut fname = PathBuf::from("/proc");
        fname.push(pid.as_ref());
        fname.push("mountinfo");

        let contents = fs::read_to_string(&fname)
            .map_err(|e| Error::file("open", &fname, e))?;

        let lines : Vec<&str> = contents.lines().collect();

        // lines like:
        // 36 35 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw,errors=continue
        // (0)(1)(2)   (3)   (4)      (5)      (6)   (7) (8)   (9)          (10)
        // where (6) may be repeated zero or more times.
        
        // order is not certain.  '/' may not be first entry
        // so we pass ignore parents on first pass

        let mut infos = HashMap::new();
        let mut parents = HashMap::new();

        for (lino, line) in lines.into_iter().enumerate() {
            let parts : Vec<&str> = line.split(' ').collect();
            if parts.len()<10 {
                return Err(Error::parse(format!("Syntax on Line {} : \"{}\"", lino+1, &line), &fname));
            }

            // find index of '-'
            let (sepidx, _) = parts.iter().enumerate().find(|(_,e)| &&"-"==e)
                .ok_or_else(|| Error::parse(format!("Missing sep in \"{}\"", &line), &fname))?;

            let id = parts[0].parse::<u64>()?;

            parents.insert(id, parts[1].parse::<u64>()?);

            infos.insert(id, rc::Rc::new(MountInfo {
                id: id,
                parent: None, // placeholder
                root: parts[3].into(),
                mount_point: parts[4].into(),
                options: parts[5].split(',').map(|o| o.to_string()).collect(),
                fstype: parts[sepidx+1].to_string(),
                source: parts[sepidx+2].to_string(),
            }));
        }

        for (id, mount) in infos.iter() {
            unsafe {
                // the tree is under our exclusive control while populating
                let cheat = mount.as_ref() as *const MountInfo as *mut MountInfo;
                (*cheat).parent = parents.get(id).and_then(|parid| infos.get(parid).map(|i| i.clone()));
            }
        }

        Ok(Mounts {
            points: infos.drain().map(|(_id, mount)| (mount.mount_point.clone(), mount)).collect(),
        })
    }

    pub fn lookup<P: AsRef<Path>>(&self, path: P) -> Result<rc::Rc<MountInfo>> {
        let mp = find_mount_point(path)?;
        self.points.get(&mp)
            .map(|info| info.clone())
            .ok_or_else(|| Error::MissingMount{})
    }
}

impl<'a> IntoIterator for &'a Mounts {
    type Item = &'a rc::Rc<MountInfo>;
    // oh for decltype()
    type IntoIter = std::collections::hash_map::Values<'a, PathBuf, rc::Rc<MountInfo>>;

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
    fn test_mountinfo() {
        let infos = Mounts::current().unwrap();
        let root = infos.lookup(&"/").unwrap();
        assert!(root.parent.is_none(), "{:?}", infos);

        for mp in &infos {
            let noroot = mp.mount_point!=Path::new("/") || mp.parent.is_some();
            assert!(mp.is_root() || noroot, "{:?} {} {}", mp, mp.is_root(), noroot);
        }
    }
}
