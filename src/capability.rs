//! Manipulate Linux process capability bit masks
use std::fmt;

use super::ext;
use libc;

pub use super::ext::CAP_SYS_ADMIN;

use super::err::{Error, Result};

#[derive(Debug, Clone, Default)]
pub struct Cap {
    pub effective: [u32; DATA_SIZE],
    pub permitted: [u32; DATA_SIZE],
    pub inheritable: [u32; DATA_SIZE],
}

const DATA_SIZE: usize = ext::_LINUX_CAPABILITY_U32S_3 as _;

fn empty_data() -> ext::__user_cap_data_struct {
    ext::__user_cap_data_struct::default()
}

impl Cap {
    /// Fetch the current capabilities of this process
    pub fn current() -> Result<Self> {
        Cap::current_pid(0)
    }

    /// Fetch the current capabilities of the specified process.  (0 for the current process)
    pub fn current_pid(pid: libc::pid_t) -> Result<Cap> {
        let mut head = ext::__user_cap_header_struct {
            version: ext::_LINUX_CAPABILITY_VERSION_3,
            pid,
        };
        let mut data = vec![empty_data(); DATA_SIZE];

        let err = unsafe { ext::capget(&mut head, data.as_mut_ptr()) };
        if err != 0 {
            return Err(Error::last_os_error("capget"));
        }

        let mut ret = Cap::default();

        for n in 0..DATA_SIZE {
            ret.effective[n] = data[n].effective;
            ret.permitted[n] = data[n].permitted;
            ret.inheritable[n] = data[n].inheritable;
        }

        Ok(ret)
    }

    /// Apply these capabilities to the current process
    pub fn update(&self) -> Result<()> {
        self.update_pid(0)
    }

    /// Apply these capabilities to the specified process.  (0 for the current process)
    pub fn update_pid(&self, pid: libc::pid_t) -> Result<()> {
        let mut data = vec![empty_data(); DATA_SIZE];

        for n in 0..DATA_SIZE {
            data[n].effective = self.effective[n];
            data[n].permitted = self.permitted[n];
            data[n].inheritable = self.inheritable[n];
        }

        let mut head = ext::__user_cap_header_struct {
            version: ext::_LINUX_CAPABILITY_VERSION_3,
            pid,
        };

        let err = unsafe { ext::capset(&mut head, data.as_mut_ptr()) };
        if err != 0 {
            return Err(Error::last_os_error("capset"));
        }
        Ok(())
    }

    /// Copy permitted mask to effective mask
    pub fn activate(&mut self) -> &mut Self {
        self.effective = self.permitted;
        self
    }

    /// Clear all bits in the effective mask
    pub fn clear_effective(&mut self) -> &mut Self {
        self.effective = [0; DATA_SIZE];
        self
    }

    /// Clear all bits in the permitted mask
    pub fn clear_permitted(&mut self) -> &mut Self {
        self.permitted = [0; DATA_SIZE];
        self
    }

    /// Clear all bits in the inheritable mask
    pub fn clear_inheritable(&mut self) -> &mut Self {
        self.inheritable = [0; DATA_SIZE];
        self
    }

    /// Clear all bit masks
    pub fn clear(&mut self) -> &mut Self {
        self.clear_effective().clear_permitted().clear_inheritable()
    }

    /// Test a bit in the effective mask
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
