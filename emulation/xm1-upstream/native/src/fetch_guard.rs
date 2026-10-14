//! Optional instruction ownership checks on the existing board engines.
//!
//! This observes fetch boundaries only. It neither authorizes data accesses nor
//! replaces CPU, scheduler, peripheral, or coprocessor behavior. Configuration
//! and sticky observations survive explicit stops and state/context restores.
use crate::engine::Inner;
use crate::ffi;

pub type Range = (u64, u64);

#[derive(Clone)]
pub struct Halt {
    pub kind: &'static str,
    pub reason: String,
    pub pc: u64,
    pub size: u32,
    pub registers: [u32; 4],
    pub opcode: Option<u32>,
}

#[derive(Clone)]
pub struct FetchGuard {
    pub ranges: Vec<Range>,
    pub vector_ranges: Vec<Range>,
    /// Sorted and pairwise disjoint: each fetch is located by binary search.
    pub ranges_disjoint: bool,
    /// Last matched index; consecutive fetches almost always share a range.
    pub last_range: usize,
    pub vector_opcode: u32,
    pub fault_address: u64,
    pub stop_addresses: Vec<u64>,
    pub checked_fetches: u64,
    pub accepted_fetches: u64,
    pub vector_fetches: u64,
    pub range_fetches: Vec<u64>,
    pub vector_range_fetches: Vec<u64>,
    pub halt: Option<Halt>,
}

fn normalize(mut ranges: Vec<Range>) -> Result<Vec<Range>, String> {
    if ranges.len() > 256 {
        return Err("Fetch guard accepts at most 256 ranges per list".into());
    }
    if ranges
        .iter()
        .any(|&(begin, end)| begin >= end || end > 0x100000000)
    {
        return Err("Fetch guard requires nonempty half-open ARM address ranges".into());
    }
    ranges.sort_unstable();
    ranges.dedup();
    Ok(ranges)
}

fn matching_range(ranges: &[Range], address: u64, size: u32) -> Option<usize> {
    let end = address.checked_add(size as u64)?;
    if size == 0 {
        return None;
    }
    ranges
        .iter()
        .position(|&(begin, limit)| address >= begin && end <= limit)
}

/// Same index as `matching_range` for sorted, pairwise-disjoint ranges, in
/// O(log n): the only candidate is the last range beginning at or below the
/// address. Holes punched by many excluded spans keep fetch checks cheap.
fn matching_disjoint(ranges: &[Range], address: u64, size: u32) -> Option<usize> {
    let end = address.checked_add(size as u64)?;
    if size == 0 {
        return None;
    }
    let index = ranges
        .partition_point(|&(begin, _)| begin <= address)
        .checked_sub(1)?;
    (end <= ranges[index].1).then_some(index)
}

fn contains(ranges: &[Range], address: u64, size: u32) -> bool {
    matching_range(ranges, address, size).is_some()
}

fn overlaps(ranges: &[Range], address: u64, size: u32) -> bool {
    address.checked_add(size as u64).is_some_and(|end| {
        ranges
            .iter()
            .any(|&(begin, limit)| address < limit && end > begin)
    })
}

impl FetchGuard {
    pub fn new(
        ranges: Vec<Range>,
        vector_ranges: Vec<Range>,
        vector_opcode: u32,
        fault_address: u64,
        mut stop_addresses: Vec<u64>,
    ) -> Result<Self, String> {
        let ranges = normalize(ranges)?;
        let vector_ranges = normalize(vector_ranges)?;
        if ranges.is_empty() && vector_ranges.is_empty() {
            return Err("Fetch guard requires at least one allowed range".into());
        }
        if fault_address > u32::MAX as u64
            || stop_addresses.iter().any(|&pc| pc > u32::MAX as u64)
            || stop_addresses.len() > 4096
        {
            return Err(
                "Fetch guard stop/fault addresses must be ARM addresses (at most 4096 stops)"
                    .into(),
            );
        }
        stop_addresses.sort_unstable();
        stop_addresses.dedup();
        let ranges_disjoint = ranges.windows(2).all(|pair| pair[0].1 <= pair[1].0);
        let range_fetches = vec![0; ranges.len()];
        let vector_range_fetches = vec![0; vector_ranges.len()];
        Ok(Self {
            ranges,
            vector_ranges,
            ranges_disjoint,
            last_range: 0,
            vector_opcode,
            fault_address,
            stop_addresses,
            checked_fetches: 0,
            accepted_fetches: 0,
            vector_fetches: 0,
            range_fetches,
            vector_range_fetches,
            halt: None,
        })
    }
}

fn halt(
    inner: &Inner,
    core: usize,
    address: u64,
    size: u32,
    kind: &'static str,
    reason: String,
    opcode: Option<u32>,
) -> Result<(), String> {
    let mut registers = [0; 4];
    for (index, value) in registers.iter_mut().enumerate() {
        *value = inner.read_reg(core, ffi::ARM_REGS[index])?;
    }
    // Drop the Rust borrow before stopping the engine. The record remains
    // sticky, including when the caller attempts to resume the same core.
    unsafe {
        let guard = (*inner.fetch_guards.get())[core].as_mut().unwrap();
        if guard.halt.is_none() {
            guard.halt = Some(Halt {
                kind,
                reason,
                pc: address,
                size,
                registers,
                opcode,
            });
        }
    }
    inner.stop(core);
    Ok(())
}

pub fn code(inner: &Inner, core: usize, address: u64, size: u32) -> Result<bool, String> {
    let decision = unsafe {
        let Some(guard) = (*inner.fetch_guards.get())[core].as_mut() else {
            return Ok(true);
        };
        if guard.halt.is_some() {
            None
        } else {
            guard.checked_fetches += 1;
            Some((
                if guard.ranges_disjoint {
                    let hint = guard.last_range;
                    let found = match guard.ranges.get(hint) {
                        Some(&(begin, limit))
                            if size != 0
                                && address >= begin
                                && address
                                    .checked_add(size as u64)
                                    .is_some_and(|end| end <= limit) =>
                        {
                            Some(hint)
                        }
                        _ => matching_disjoint(&guard.ranges, address, size),
                    };
                    if let Some(index) = found {
                        guard.last_range = index;
                    }
                    found
                } else {
                    matching_range(&guard.ranges, address, size)
                },
                overlaps(&guard.vector_ranges, address, size),
                matching_range(&guard.vector_ranges, address, size),
                guard.vector_opcode,
                guard.fault_address != 0 && address == guard.fault_address,
                guard.stop_addresses.binary_search(&address).is_ok(),
            ))
        }
    };
    let Some((range_index, vector, vector_index, expected, fault, stop)) = decision else {
        inner.stop(core);
        return Ok(false);
    };
    if range_index.is_none() && vector_index.is_none() {
        halt(
            inner,
            core,
            address,
            size,
            "blocked",
            format!("Instruction fetch outside allowed ranges at {address:#x} (width {size})"),
            None,
        )?;
        return Ok(false);
    }
    let opcode = if vector {
        if size != 4 || vector_index.is_none() || !address.is_multiple_of(4) {
            halt(
                inner,
                core,
                address,
                size,
                "blocked",
                format!(
                    "Copied vector instruction has invalid bounds/width/alignment at {address:#x}"
                ),
                None,
            )?;
            return Ok(false);
        }
        let bytes = inner.read_cpu(core, address, 4)?;
        let opcode = u32::from_le_bytes(bytes.try_into().unwrap());
        if opcode != expected {
            halt(
                inner,
                core,
                address,
                size,
                "blocked",
                format!(
                    "Copied vector opcode mismatch at {address:#x}: expected {expected:#010x}, observed {opcode:#010x}"
                ),
                Some(opcode),
            )?;
            return Ok(false);
        }
        Some(opcode)
    } else {
        None
    };
    if fault {
        let kind = inner.read_reg(core, ffi::ARM_REGS[0])?;
        let target = inner.read_reg(core, ffi::ARM_REGS[1])?;
        halt(
            inner,
            core,
            address,
            size,
            "fault",
            format!("C fault kind={kind} address={target:#x}"),
            opcode,
        )?;
        return Ok(false);
    }
    if stop {
        halt(
            inner,
            core,
            address,
            size,
            "stop",
            format!("Configured instruction stop at {address:#x}"),
            opcode,
        )?;
        return Ok(false);
    }
    unsafe {
        let guard = (*inner.fetch_guards.get())[core].as_mut().unwrap();
        guard.accepted_fetches += 1;
        guard.vector_fetches += u64::from(vector);
        if let Some(index) = range_index {
            guard.range_fetches[index] += 1;
        }
        if let Some(index) = vector_index {
            guard.vector_range_fetches[index] += 1;
        }
    }
    Ok(true)
}

/// Unicorn can reject an unmapped/protected fetch before invoking its code
/// hook. Record that rejection without asking a device callback to fix it.
/// Data-access faults retain the normal board callback path.
pub fn invalid_fetch(
    inner: &Inner,
    core: usize,
    kind: i32,
    address: u64,
    size: i32,
) -> Result<bool, String> {
    if !matches!(kind, 21 | 24) {
        return Ok(false);
    }
    let width = u32::try_from(size).unwrap_or(0);
    let allowed = unsafe {
        let Some(guard) = (*inner.fetch_guards.get())[core].as_mut() else {
            return Ok(false);
        };
        if guard.halt.is_some() {
            None
        } else {
            guard.checked_fetches += 1;
            Some(
                contains(&guard.ranges, address, width)
                    || contains(&guard.vector_ranges, address, width),
            )
        }
    };
    let Some(allowed) = allowed else {
        inner.stop(core);
        return Ok(true);
    };
    let (category, reason) = if allowed {
        (
            "fetch_error",
            format!("Unmapped/protected instruction fetch {kind} at {address:#x}"),
        )
    } else {
        (
            "blocked",
            format!(
                "Instruction fetch outside allowed ranges at {address:#x} (width {width}; unmapped/protected {kind})"
            ),
        )
    };
    halt(inner, core, address, width, category, reason, None)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_normalization_and_width() {
        let ranges = normalize(vec![(12, 16), (0, 8), (8, 12)]).unwrap();
        assert_eq!(ranges, vec![(0, 8), (8, 12), (12, 16)]);
        assert!(contains(&ranges, 12, 4));
        assert!(!contains(&ranges, 13, 4));
        assert!(!contains(&ranges, 6, 4));
        assert!(!contains(&ranges, 0, 0));
        assert!(!contains(&ranges, u64::MAX, 4));
        assert!(contains(&[(0, 0x100000000)], 0xfffffffc, 4));
        assert!(!contains(&[(0, 0x100000000)], 0xfffffffe, 4));
        let holes = vec![(0, 8), (16, 24), (24, 40), (64, 0x100000000)];
        for address in (0..80).chain([0xfffffff8, 0xfffffffc, 0xfffffffe]) {
            for size in [0, 2, 4] {
                assert_eq!(
                    matching_disjoint(&holes, address, size),
                    matching_range(&holes, address, size)
                );
            }
        }
    }

    #[test]
    fn invalid_configurations() {
        assert!(FetchGuard::new(vec![], vec![], 0, 0, vec![]).is_err());
        for ranges in [
            vec![(1, 1)],
            vec![(2, 1)],
            vec![(0, 0x100000001)],
            vec![(0, 4); 257],
        ] {
            assert!(FetchGuard::new(ranges, vec![], 0, 0, vec![]).is_err());
        }
        assert!(FetchGuard::new(vec![(0, 4)], vec![], 0, 0x100000000, vec![]).is_err());
        assert!(FetchGuard::new(vec![(0, 4)], vec![], 0, 0, vec![0x100000000]).is_err());
    }
}
