//! Opt-in per-routine observation on the existing native Unicorn engines.
//!
//! No audit borrow survives a stop/FFI/Python event boundary. Disabled auditing
//! does not change native instruction, device, or store semantics.
use crate::engine::Inner;
use crate::ffi;
use std::collections::BTreeSet;

pub type Write = (u64, u32, u64, u64);

#[derive(Clone)]
pub struct Audit {
    pub stack_start: u64,
    pub stack_end: u64,
    pub text_begin: u64,
    pub text_end: u64,
    pub data_begin: u64,
    pub data_end: u64,
    pub fault_address: u64,
    pub trace: bool,
    pub instructions: u64,
    pub minimum_sp: u64,
    pub failure: Option<String>,
    pub writes: Vec<Write>,
    pub dirty_pages: BTreeSet<u64>,
    pub pcs: Vec<u64>,
}

impl Audit {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ram_size: u64,
        stack_start: u64,
        stack_end: u64,
        text_begin: u64,
        text_end: u64,
        fault_address: u64,
        trace: bool,
        data_begin: u64,
        data_end: u64,
    ) -> Result<Self, String> {
        if stack_start >= stack_end || stack_end > ram_size {
            return Err("Audit stack exclusion must be a nonempty owned-DRAM range".into());
        }
        for (name, begin, end) in [
            ("text", text_begin, text_end),
            ("data", data_begin, data_end),
        ] {
            if (begin != 0 || end != 0) && (begin >= end || end > 0x100000000) {
                return Err(format!("Invalid audit {name} bounds"));
            }
        }
        if fault_address > u32::MAX as u64 {
            return Err("Audit fault symbol exceeds the ARM address space".into());
        }
        Ok(Self {
            stack_start,
            stack_end,
            text_begin,
            text_end,
            data_begin,
            data_end,
            fault_address,
            trace,
            instructions: 0,
            minimum_sp: stack_end,
            failure: None,
            writes: Vec::new(),
            dirty_pages: BTreeSet::new(),
            pcs: Vec::new(),
        })
    }
}

pub fn enabled(inner: &Inner, core: usize) -> bool {
    unsafe { (*inner.audits.get())[core].is_some() }
}

pub fn fail(inner: &Inner, core: usize, message: String) {
    unsafe {
        if let Some(audit) = &mut (*inner.audits.get())[core]
            && audit.failure.is_none()
        {
            audit.failure = Some(message);
        }
    }
    inner.stop(core);
}

pub fn blocked(inner: &Inner, core: usize) -> bool {
    let blocked = unsafe {
        (*inner.audits.get())[core]
            .as_ref()
            .is_some_and(|a| a.failure.is_some())
    };
    if blocked {
        inner.stop(core);
    }
    blocked
}

fn contains(begin: u64, end: u64, address: u64, size: u64) -> bool {
    begin < end
        && address >= begin
        && size != 0
        && address.checked_add(size).is_some_and(|limit| limit <= end)
}

fn ram_address(address: u64, size: u64, ram_size: u64) -> Option<u64> {
    if contains(0, ram_size, address, size) {
        Some(address)
    } else if contains(0x40000000, 0x40000000 + ram_size, address, size) {
        Some(address - 0x40000000)
    } else {
        None
    }
}

pub fn code(inner: &Inner, core: usize, address: u64, size: u32) -> Result<bool, String> {
    if !enabled(inner, core) {
        return Ok(true);
    }
    if blocked(inner, core) {
        return Ok(false);
    }
    let raw_sp = inner.read_reg(core, ffi::ARM_REGS[13])? as u64;
    let sp = if (0x40000000..=0x40000000 + inner.ram.len() as u64).contains(&raw_sp) {
        raw_sp - 0x40000000
    } else {
        raw_sp
    };
    let is_fault = unsafe {
        (*inner.audits.get())[core]
            .as_ref()
            .is_some_and(|a| a.fault_address != 0 && address == a.fault_address)
    };
    let fault = if is_fault {
        Some(format!(
            "C fault kind={} address={:#x}",
            inner.read_reg(core, ffi::ARM_REGS[0])?,
            inner.read_reg(core, ffi::ARM_REGS[1])?
        ))
    } else {
        None
    };
    let failure = unsafe {
        let audit = (*inner.audits.get())[core].as_mut().unwrap();
        audit.instructions += 1;
        audit.minimum_sp = audit.minimum_sp.min(sp);
        if audit.trace && audit.pcs.len() < 4096 {
            audit.pcs.push(address);
        }
        if fault.is_some() {
            fault
        } else if audit.text_begin != audit.text_end
            && !contains(audit.text_begin, audit.text_end, address, size as u64)
        {
            Some(format!("C executed outside its own text at {address:#x}"))
        } else if sp < audit.stack_start || sp > audit.stack_end {
            Some("Stack exceeded the explicit scratch exclusion".into())
        } else {
            None
        }
    };
    if let Some(message) = failure {
        fail(inner, core, message);
        Ok(false)
    } else {
        Ok(true)
    }
}

pub fn read(inner: &Inner, core: usize, address: u64, size: i32) -> bool {
    if !enabled(inner, core) {
        return true;
    }
    if blocked(inner, core) {
        return false;
    }
    let allowed = size > 0
        && (ram_address(address, size as u64, inner.ram.len() as u64).is_some()
            || unsafe {
                let audit = (*inner.audits.get())[core].as_ref().unwrap();
                contains(audit.text_begin, audit.text_end, address, size as u64)
                    || contains(audit.data_begin, audit.data_end, address, size as u64)
            });
    if !allowed {
        fail(inner, core, format!("Unmodeled data read at {address:#x}"));
    }
    allowed
}

pub fn write(
    inner: &Inner,
    core: usize,
    address: u64,
    size: i32,
    value: i64,
) -> Result<bool, String> {
    if !enabled(inner, core) {
        return Ok(true);
    }
    if blocked(inner, core) {
        return Ok(false);
    }
    let Some(physical) = (size > 0)
        .then(|| ram_address(address, size as u64, inner.ram.len() as u64))
        .flatten()
    else {
        fail(inner, core, format!("Unmodeled write at {address:#x}"));
        return Ok(false);
    };
    let pc = inner.read_reg(core, ffi::PC)? as u64;
    let failure = unsafe {
        let audit = (*inner.audits.get())[core].as_mut().unwrap();
        let end = physical + size as u64;
        audit.dirty_pages.extend(physical >> 12..=(end - 1) >> 12);
        if !(1..=8).contains(&size) {
            Some(format!("Unsupported audited write width {size}"))
        } else if physical >= audit.stack_start && end <= audit.stack_end {
            None
        } else if physical < audit.stack_end && end > audit.stack_start {
            Some("Write straddles the stack scratch exclusion".into())
        } else {
            audit.writes.push((physical, size as u32, value as u64, pc));
            None
        }
    };
    if let Some(message) = failure {
        fail(inner, core, message);
        Ok(false)
    } else {
        Ok(true)
    }
}

/// The existing ordinary-store hooks cover these address starts. The global
/// guard only audits writes elsewhere, so owned-RAM stores are collected once.
pub fn ordinary_store_address(address: u64, ram_size: u64) -> bool {
    address < ram_size
        || (0x40000000..0x40000000 + ram_size).contains(&address)
        || (0xfff00000..0xfff01000).contains(&address)
        || (0x3fff0000..0x3fff1000).contains(&address)
        || (0x7fff0000..0x7fff1000).contains(&address)
        || (0xffff0000..=0xffffffff).contains(&address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_aliases_are_bounded_and_private_mappings_are_rejected() {
        assert_eq!(ram_address(0x1fff, 1, 0x2000), Some(0x1fff));
        assert_eq!(ram_address(0x40001ffc, 4, 0x2000), Some(0x1ffc));
        assert_eq!(ram_address(0x40001fff, 2, 0x2000), None);
        assert_eq!(ram_address(0x3fff0000, 4, 0x2000), None);
        assert_eq!(ram_address(0xffff0000, 4, 0x2000), None);
        assert_eq!(ram_address(u64::MAX, 4, 0x2000), None);
    }

    #[test]
    fn configuration_checks_stack_text_and_data_bounds() {
        assert!(Audit::new(0x2000, 0x1000, 0x2000, 0, 0, 0, false, 0, 0).is_ok());
        assert!(Audit::new(0x2000, 0x2000, 0x1000, 0, 0, 0, false, 0, 0).is_err());
        assert!(Audit::new(0x2000, 0, 0x3000, 0, 0, 0, false, 0, 0).is_err());
        assert!(Audit::new(0x2000, 0, 0x2000, 4, 4, 0, false, 0, 0).is_err());
        assert!(Audit::new(0x2000, 0, 0x2000, 0, 0, 0, false, 8, 4).is_err());
    }
}
