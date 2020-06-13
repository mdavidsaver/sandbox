
use std::io::Error;

use libc;
use super::ext;

#[derive(Debug, Clone)]
pub struct Cap {
    pub effective: Vec<bool>,
    pub permitted: Vec<bool>,
    pub inheritable: Vec<bool>,
}

/// Get Linux capabilities for specific process (or 0 for self)
pub fn capget(pid: libc::pid_t) -> Result<Cap, Error> {
    let mut head = ext::__user_cap_header_struct {
        version: ext::_LINUX_CAPABILITY_VERSION_3,
        pid: pid,
    };
    let mut data = vec![ext::__user_cap_data_struct{effective:0,inheritable:0,permitted:0}; 4*ext::_LINUX_CAPABILITY_U32S_3 as usize];

    let err = unsafe {
        ext::capget(&mut head, data.as_mut_ptr())
    };
    if err!=0 {
        return Err(Error::last_os_error());
    }

    let nbits = 32 * ext::_LINUX_CAPABILITY_U32S_3 as usize;

    let mut ret = Cap {
        effective: vec![false; nbits],
        permitted: vec![false; nbits],
        inheritable: vec![false; nbits],
    };

    for n in 0..nbits {
        let i = n/32;
        let m = 1<<(n%32) as u32;
        if (data[i].effective&m)!=0 {
            ret.effective[n] = true;
        }
        if (data[i].permitted&m)!=0 {
            ret.permitted[n] = true;
        }
        if (data[i].inheritable&m)!=0 {
            ret.inheritable[n] = true;
        }
    }

    Ok(ret)
}

/// Clear Linux capabilities for this process
pub fn capclear() -> Result<(), Error> {
    let mut head = ext::__user_cap_header_struct {
        version: ext::_LINUX_CAPABILITY_VERSION_3,
        pid: 0,
    };
    let mut data = vec![ext::__user_cap_data_struct{effective:0,inheritable:0,permitted:0}; ext::_LINUX_CAPABILITY_U32S_3 as usize];

    let err = unsafe {
        ext::capset(&mut head, data.as_mut_ptr())
    };
    if err!=0 {
        return Err(Error::last_os_error());
    }

    let err = unsafe {
        libc::prctl(libc::PR_CAP_AMBIENT, libc::PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0)
    };
    if err!=0 {
        return Err(Error::last_os_error());
    }

    Ok(())
}
