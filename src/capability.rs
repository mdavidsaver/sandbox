use std::fmt;

use super::ext;
use libc;

pub use super::ext::CAP_SYS_ADMIN;

use super::err::{Error, Result};

#[derive(Debug, Clone)]
pub struct Cap {
    pub effective: Vec<u32>,
    pub permitted: Vec<u32>,
    pub inheritable: Vec<u32>,
}

const DATA_SIZE: usize = ext::_LINUX_CAPABILITY_U32S_3 as usize;

fn empty_data() -> ext::__user_cap_data_struct {
    ext::__user_cap_data_struct::default()
}

impl Cap {
    pub fn current() -> Result<Cap> {
        Cap::current_pid(0)
    }

    pub fn current_pid(pid: libc::pid_t) -> Result<Cap> {
        let mut head = ext::__user_cap_header_struct {
            version: ext::_LINUX_CAPABILITY_VERSION_3,
            pid: pid,
        };
        let mut data = vec![empty_data(); DATA_SIZE];

        let err = unsafe { ext::capget(&mut head, data.as_mut_ptr()) };
        if err != 0 {
            return Err(Error::last_os_error("capget"));
        }

        let mut ret = Cap {
            effective: vec![0; DATA_SIZE],
            permitted: vec![0; DATA_SIZE],
            inheritable: vec![0; DATA_SIZE],
        };

        for n in 0..DATA_SIZE {
            ret.effective[n] = data[n].effective;
            ret.permitted[n] = data[n].permitted;
            ret.inheritable[n] = data[n].inheritable;
        }

        Ok(ret)
    }

    pub fn update(&self) -> Result<()> {
        self.update_pid(0)
    }

    pub fn update_pid(&self, pid: libc::pid_t) -> Result<()> {
        let mut data = vec![empty_data(); DATA_SIZE];

        for n in 0..DATA_SIZE {
            data[n].effective = self.effective[n];
            data[n].permitted = self.permitted[n];
            data[n].inheritable = self.inheritable[n];
        }

        let mut head = ext::__user_cap_header_struct {
            version: ext::_LINUX_CAPABILITY_VERSION_3,
            pid: pid,
        };

        let err = unsafe { ext::capset(&mut head, data.as_mut_ptr()) };
        if err != 0 {
            return Err(Error::last_os_error("capset"));
        }
        Ok(())
    }

    /// Copy permitted to effective
    pub fn activate(&mut self) -> &mut Self {
        for i in 0..self.effective.len() {
            self.effective[i] = self.permitted[i];
        }
        self
    }

    pub fn clear_effective(&mut self) -> &mut Self {
        for i in 0..self.effective.len() {
            self.effective[i] = 0;
        }
        self
    }

    pub fn clear_permitted(&mut self) -> &mut Self {
        for i in 0..self.permitted.len() {
            self.permitted[i] = 0;
        }
        self
    }

    pub fn clear_inheritable(&mut self) -> &mut Self {
        for i in 0..self.inheritable.len() {
            self.inheritable[i] = 0;
        }
        self
    }

    pub fn clear(&mut self) -> &mut Self {
        self.clear_effective().clear_permitted().clear_inheritable()
    }

    pub fn effective(&self, cap: u32) -> bool {
        let word = cap / 32;
        let bit = cap % 32;
        0 != (self.effective[word as usize] & bit)
    }
}

fn fmt_arr(arr: &[u32], f: &mut fmt::Formatter<'_>) -> fmt::Result {
    for n in (0..arr.len()).rev() {
        write!(f, "{:08x}", arr[n])?;
    }
    Ok(())
}

impl fmt::Display for Cap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CapEff: 0x")?;
        fmt_arr(&self.effective, f)?;
        write!(f, " CapPrm: 0x")?;
        fmt_arr(&self.permitted, f)?;
        write!(f, " CapInh: 0x")?;
        fmt_arr(&self.inheritable, f)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_current() {
        Cap::current().unwrap();
    }

    #[test]
    fn apply_current() {
        Cap::current().unwrap().update().unwrap();
    }
}
