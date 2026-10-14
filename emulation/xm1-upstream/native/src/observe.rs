//! Opt-in routine-call observers: per-core breakpoints and a bounded store log.
//!
//! Both are empty by default; hooks test one emptiness condition before doing
//! any work. Neither is checkpoint state, neither calls Python for stores, and
//! configuration is refused while an engine executes.
use crate::engine::Inner;
use std::collections::HashMap;

/// `(physical_or_canonical_address, width, value, pc)`: the stable, original
/// ordered store tuple.
pub type Store = (u64, u32, u64, u64);
/// Store with guest virtual address and flags (`FLAG_*`).
pub type Record = (u64, u32, u64, u64, u64, u32);

pub const FLAG_STACK: u32 = 1;
pub const FLAG_EXCLUSIVE: u32 = 2;
pub const FLAG_STRADDLE: u32 = 4;

pub struct StoreLog {
    pub stack_start: u64,
    pub stack_end: u64,
    pub allowed: Option<Vec<(u64, u64)>>,
    pub limit: usize,
    pub include_stack: bool,
    pub records: Vec<Record>,
    pub violation: Option<String>,
}

impl StoreLog {
    pub fn new(
        stack_start: u64,
        stack_end: u64,
        allowed: Option<Vec<(u64, u64)>>,
        limit: usize,
        include_stack: bool,
    ) -> Result<Self, String> {
        // The window is in canonical address space: owned DRAM offsets, or the
        // calling core's TCM addresses (up to the top of the 32-bit space).
        if stack_start >= stack_end || stack_end > 0x1_0000_0000 {
            return Err("Store log stack exclusion must be a nonempty ARM address range".into());
        }
        if limit == 0 || limit > 1 << 24 {
            return Err("Store log limit must be 1..16777216 entries".into());
        }
        if let Some(ranges) = &allowed
            && (ranges.len() > 4096 || ranges.iter().any(|&(lo, hi)| lo >= hi))
        {
            return Err("Allowed stores need at most 4096 nonempty half-open ranges".into());
        }
        Ok(Self {
            stack_start,
            stack_end,
            allowed,
            limit,
            include_stack,
            records: Vec::new(),
            violation: None,
        })
    }
}

/// Owned DRAM and its 0x40000000 alias are reported by physical offset; stores
/// to the other store-hooked regions (SRAM, vector copies) keep their address.
fn canonical(address: u64, ram_size: u64) -> u64 {
    if (0x40000000..0x40000000 + ram_size).contains(&address) {
        address - 0x40000000
    } else {
        address
    }
}

/// Executed-instruction set: one 2-byte-granular bitmap per 4 KiB page. The
/// last page is cached so straight-line and loop code never hashes.
pub struct Coverage {
    index: HashMap<u64, usize>,
    pages: Vec<(u64, [u64; 32])>,
    last: usize,
}

impl Default for Coverage {
    fn default() -> Self {
        Self::new()
    }
}

impl Coverage {
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
            pages: Vec::new(),
            last: usize::MAX,
        }
    }
    pub fn insert(&mut self, address: u64) {
        let page = address >> 12;
        let slot = if self.last < self.pages.len() && self.pages[self.last].0 == page {
            self.last
        } else {
            let next = self.pages.len();
            let slot = *self.index.entry(page).or_insert(next);
            if slot == next {
                self.pages.push((page, [0; 32]));
            }
            self.last = slot;
            slot
        };
        let bit = ((address & 0xfff) >> 1) as usize;
        self.pages[slot].1[bit >> 6] |= 1 << (bit & 63);
    }
    /// Sorted unique addresses.
    pub fn addresses(&self) -> Vec<u64> {
        let mut pages: Vec<&(u64, [u64; 32])> = self.pages.iter().collect();
        pages.sort_unstable_by_key(|entry| entry.0);
        let mut result = Vec::new();
        for (page, words) in pages {
            for (word_index, word) in words.iter().enumerate() {
                let mut bits = *word;
                while bits != 0 {
                    let bit = bits.trailing_zeros() as u64;
                    bits &= bits - 1;
                    result.push((page << 12) | (((word_index as u64) << 6 | bit) << 1));
                }
            }
        }
        result
    }
}

/// Opt-in per-call execution probe: exact executed-instruction counts for
/// caller-chosen ranges and the first `trace_limit` executed PCs in order.
/// Both are updated on the same path as coverage (an instruction that is
/// counted in `State::instructions`), so a stub, stop or redirecting observer
/// hit whose instruction does not execute is never counted or traced.
/// `(range_counts, trace, trace_total)`.
pub type ProbeState = (Vec<u64>, Vec<u64>, u64);

pub struct ExecProbe {
    ranges: Vec<(u64, u64)>,
    pub counts: Vec<u64>,
    pub trace_limit: usize,
    pub trace: Vec<u64>,
    pub trace_total: u64,
    ram_size: u64,
}

impl ExecProbe {
    pub const MAX_RANGES: usize = 256;
    pub const MAX_TRACE: usize = 1 << 20;

    /// `ranges` are half-open ranges in canonical address space (DRAM offsets;
    /// executed addresses through the 0x40000000 alias are folded onto them).
    pub fn new(ranges: Vec<(u64, u64)>, trace_limit: usize, ram_size: u64) -> Result<Self, String> {
        if ranges.len() > Self::MAX_RANGES
            || ranges
                .iter()
                .any(|&(lo, hi)| lo >= hi || hi > 0x1_0000_0000)
        {
            return Err("Count ranges need at most 256 nonempty half-open ARM ranges".into());
        }
        if trace_limit > Self::MAX_TRACE {
            return Err("Trace bound must be at most 1048576 PCs".into());
        }
        let counts = vec![0; ranges.len()];
        Ok(Self {
            ranges,
            counts,
            trace_limit,
            trace: Vec::with_capacity(trace_limit.min(4096)),
            trace_total: 0,
            ram_size,
        })
    }
    pub fn record(&mut self, address: u64) {
        if !self.ranges.is_empty() {
            let canonical = canonical(address, self.ram_size);
            for (count, &(lo, hi)) in self.counts.iter_mut().zip(&self.ranges) {
                if canonical >= lo && canonical < hi {
                    *count += 1;
                }
            }
        }
        if self.trace_limit != 0 {
            self.trace_total += 1;
            if self.trace.len() < self.trace_limit {
                self.trace.push(address);
            }
        }
    }
}

pub fn enabled(inner: &Inner, core: usize) -> bool {
    unsafe { (*inner.store_logs.get())[core].is_some() }
}

/// Record one store (ordinary hooked store, or a successful exclusive store
/// with `exclusive`). A violation or overflow is sticky, stops the engine, and
/// is reported by the caller from `store_log_state`.
pub fn record(
    inner: &Inner,
    core: usize,
    address: u64,
    size: i32,
    value: i64,
    pc: u64,
    exclusive: bool,
) {
    let ram_size = inner.ram.len() as u64;
    let physical = canonical(address, ram_size);
    let width = size.max(0) as u64;
    let mask = u64::MAX >> (64 - 8 * width.clamp(1, 8));
    let extra = if exclusive { FLAG_EXCLUSIVE } else { 0 };
    let stop = unsafe {
        let Some(log) = (*inner.store_logs.get())[core].as_mut() else {
            return;
        };
        if log.violation.is_some() {
            true
        } else {
            let end = physical.saturating_add(width);
            let inside = physical >= log.stack_start && end <= log.stack_end;
            let straddle = !inside && physical < log.stack_end && end > log.stack_start;
            let flags = extra
                | if inside { FLAG_STACK } else { 0 }
                | if straddle { FLAG_STRADDLE } else { 0 };
            if straddle && !log.include_stack {
                log.violation = Some(format!(
                    "Store at {physical:#x} width {width} (pc {pc:#x}) straddles the caller stack window"
                ));
                true
            } else if inside && !log.include_stack {
                false
            } else if !inside
                && !straddle
                && log.allowed.as_ref().is_some_and(|allowed| {
                    !allowed.iter().any(|&(lo, hi)| physical >= lo && end <= hi)
                })
            {
                log.violation = Some(format!(
                    "Store outside allowed ranges at {physical:#x} width {width} value {:#x} (pc {pc:#x})",
                    value as u64 & mask
                ));
                true
            } else if log.records.len() >= log.limit {
                log.violation = Some(format!(
                    "Store log exceeded its {} entry bound at pc {pc:#x}",
                    log.limit
                ));
                true
            } else {
                log.records.push((
                    physical,
                    width as u32,
                    value as u64 & mask,
                    pc,
                    address,
                    flags,
                ));
                false
            }
        }
    };
    if stop {
        inner.stop(core);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_checks_stack_limit_and_ranges() {
        assert!(StoreLog::new(0x1000, 0x2000, None, 1, false).is_ok());
        assert!(StoreLog::new(0xffff_8000, 0x1_0000_0000, None, 1, true).is_ok());
        assert!(StoreLog::new(0x2000, 0x1000, None, 1, false).is_err());
        assert!(StoreLog::new(0, 0x1_0000_0001, None, 1, false).is_err());
        assert!(StoreLog::new(0, 0x1000, None, 0, false).is_err());
        assert!(StoreLog::new(0, 0x1000, Some(vec![(4, 4)]), 1, false).is_err());
        assert!(StoreLog::new(0, 0x1000, Some(vec![(4, 8)]), 1, false).is_ok());
    }

    #[test]
    fn coverage_is_sorted_unique_across_pages() {
        let mut coverage = Coverage::new();
        for address in [0x2004, 0x1000, 0x2004, 0x1ffc, 0xffff_fffc, 0x2000, 0x1004] {
            coverage.insert(address);
        }
        assert_eq!(
            coverage.addresses(),
            vec![0x1000, 0x1004, 0x1ffc, 0x2000, 0x2004, 0xffff_fffc]
        );
    }

    #[test]
    fn exec_probe_counts_ranges_and_orders_trace() {
        assert!(ExecProbe::new(vec![(4, 4)], 0, 0x2000).is_err());
        assert!(ExecProbe::new(vec![], ExecProbe::MAX_TRACE + 1, 0x2000).is_err());
        let mut probe =
            ExecProbe::new(vec![(0x1000, 0x1008), (0x1004, 0x1010)], 3, 0x2000).unwrap();
        for address in [0x1004, 0x40001000, 0x1004, 0x1010, 0x2000] {
            probe.record(address);
        }
        assert_eq!(probe.counts, vec![3, 2]);
        assert_eq!(probe.trace, vec![0x1004, 0x40001000, 0x1004]);
        assert_eq!(probe.trace_total, 5);
    }

    #[test]
    fn dram_alias_reports_physical_offsets() {
        assert_eq!(canonical(0x40001000, 0x2000), 0x1000);
        assert_eq!(canonical(0x40002000, 0x2000), 0x40002000);
        assert_eq!(canonical(0xfff00010, 0x2000), 0xfff00010);
    }
}
