//! Passive, bounded access records over canonical DRAM ranges.
//! No firmware/model addresses and no response, memory or register mutation.
use crate::engine::Inner;

#[derive(Clone, Debug)]
pub struct Access {
    pub source: &'static str,
    pub core: usize,
    pub pc: u64,
    pub address: u64,
    pub physical: u64,
    pub size: u64,
    pub instructions: u64,
    pub ticks: u64,
}

pub struct AccessProbe {
    pub ranges: Vec<(u64, u64)>,
    pub records: Vec<Access>,
    pub limit: usize,
    pub violation: Option<String>,
    pub current_pc: [u64; 2],
}

impl AccessProbe {
    pub fn new(mut ranges: Vec<(u64, u64)>, limit: usize, ram_size: u64) -> Result<Self, String> {
        if ranges.is_empty() || ranges.len() > 256 || limit == 0 || limit > 4_194_304 {
            return Err("Access probe requires bounded nonempty ranges/record limit".into());
        }
        ranges.sort_unstable();
        if ranges.iter().any(|&(lo, hi)| lo >= hi || hi > ram_size)
            || ranges.windows(2).any(|p| p[0].1 > p[1].0)
        {
            return Err("Access probe ranges must be nonoverlapping canonical DRAM extents".into());
        }
        Ok(Self { ranges, records: Vec::new(), limit, violation: None, current_pc: [0; 2] })
    }

    pub fn matches(&self, address: u64, size: u64, ram_size: u64) -> Option<u64> {
        // The same fixed aliases as the native machine; no arbitrary bit fold.
        let physical = if address < ram_size {
            address
        } else if (0x40000000..0x40000000 + ram_size).contains(&address) {
            address - 0x40000000
        } else {
            return None;
        };
        let end = physical.checked_add(size)?;
        (size != 0 && self.ranges.iter().any(|&(lo, hi)| physical < hi && lo < end))
            .then_some(physical)
    }
}

pub fn enabled(inner: &Inner) -> bool {
    unsafe { (*inner.access_probe.get()).is_some() }
}

pub fn record(inner: &Inner, core: usize, address: u64, size: u64,
              source: &'static str, counters: Option<(u64, u64)>) -> Result<(), String> {
    let physical = unsafe {
        let Some(probe) = (*inner.access_probe.get()).as_ref() else { return Ok(()); };
        let Some(physical) = probe.matches(address, size, inner.ram.len() as u64) else { return Ok(()); };
        physical
    };
    let pc = if source.starts_with("cpu-") {
        unsafe { (*inner.access_probe.get()).as_ref().unwrap().current_pc[core] }
    } else {
        inner.read_reg(core, crate::ffi::PC)? as u64
    };
    let (instructions, ticks) = counters.unwrap_or_else(|| unsafe {
        let state = &*inner.state.get();
        (state.instructions, state.ticks)
    });
    let probe = unsafe { (*inner.access_probe.get()).as_mut().unwrap() };
    if let Some(error) = &probe.violation {
        return Err(error.clone());
    }
    if probe.records.len() == probe.limit {
        let error = format!("Access probe exceeded its {} record bound at core{core} PC{pc:#x}", probe.limit);
        probe.violation = Some(error.clone());
        return Err(error);
    }
    probe.records.push(Access { source, core, pc, address, physical, size, instructions, ticks });
    Ok(())
}

pub fn fetch(inner: &Inner, core: usize, address: u64, size: u64) -> Result<(), String> {
    if let Some(probe) = unsafe { (*inner.access_probe.get()).as_mut() } {
        probe.current_pc[core] = address;
    }
    record(inner, core, address, size, "cpu-fetch", None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_overlap_aliases_and_refusals() {
        let p = AccessProbe::new(vec![(0x1000, 0x1010)], 2, 0x2000).unwrap();
        assert_eq!(p.matches(0xffc, 8, 0x2000), Some(0xffc));
        assert_eq!(p.matches(0x40000ffc, 8, 0x2000), Some(0xffc));
        assert_eq!(p.matches(0x1010, 4, 0x2000), None);
        assert_eq!(p.matches(0xffff1000, 4, 0x2000), None);
        assert!(AccessProbe::new(vec![(0, 8), (4, 16)], 1, 0x2000).is_err());
        assert!(AccessProbe::new(vec![], 1, 0x2000).is_err());
        assert!(AccessProbe::new(vec![(0, 8)], 0, 0x2000).is_err());
    }
}
