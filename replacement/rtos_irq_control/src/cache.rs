//! Stock `0x425c8..0x427b4`: SCTLR cache enables and the four range maintenance entries.

use super::shift_left_register;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Maintenance {
    /// ICIALLU `c7,c5,0`.
    InvalidateInstructionAll,
    /// ICIMVAU `c7,c5,1`.
    InvalidateInstructionLine,
    /// DCIMVAC `c7,c6,1`.
    InvalidateDataLine,
    /// DCCMVAC `c7,c10,1`.
    CleanDataLine,
    /// DCCSW `c7,c10,2`.
    CleanDataSetWay,
    /// DCCIMVAC `c7,c14,1`.
    CleanInvalidateDataLine,
    /// DCCISW `c7,c14,2`.
    CleanInvalidateDataSetWay,
    /// Cortex-R4 `c15,c5,0`.
    InvalidateDataAll,
}

pub trait Cp15 {
    fn read_sctlr(&self) -> u32;
    fn write_sctlr(&self, value: u32);
    fn read_clidr(&self) -> u32;
    fn write_csselr(&self, value: u32);
    fn read_ccsidr(&self) -> u32;
    fn maintain(&self, op: Maintenance, operand: u32);
    fn dsb(&self);
    fn isb(&self);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cache {
    Instruction,
    Data,
}

impl Cache {
    /// SCTLR.I or SCTLR.C.
    pub const fn sctlr_bit(self) -> u32 {
        match self {
            Cache::Instruction => 0x1000,
            Cache::Data => 0x4,
        }
    }
}

/// Stock `0x425c8`/`0x425d0`. Returns the SCTLR bit, stock's r0. Each invalidation is an `mcreq`
/// after `tst r0, #bit`, so enabling the I-cache invalidates the data cache and enabling the
/// D-cache invalidates the instruction cache.
#[inline(always)]
pub fn enable<C: Cp15>(c: &C, cache: Cache) -> u32 {
    let bit = cache.sctlr_bit();
    let sctlr = c.read_sctlr() | bit;
    c.dsb();
    match cache {
        Cache::Instruction => c.maintain(Maintenance::InvalidateDataAll, 0),
        Cache::Data => c.maintain(Maintenance::InvalidateInstructionAll, 0),
    }
    c.write_sctlr(sctlr);
    c.isb();
    bit
}

/// Stock `0x425d8`/`0x425e0`. Returns the SCTLR bit, stock's r0.
#[inline(always)]
pub fn disable<C: Cp15>(c: &C, cache: Cache) -> u32 {
    let bit = cache.sctlr_bit();
    let sctlr = c.read_sctlr() & !bit;
    c.dsb();
    c.write_sctlr(sctlr);
    c.isb();
    bit
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RangeOp {
    /// `0x4262c`.
    InvalidateInstruction,
    /// `0x42634`.
    CleanInvalidateData,
    /// `0x4263c`.
    CleanData,
    /// `0x42644`.
    InvalidateData,
}

impl RangeOp {
    const fn line(self) -> Maintenance {
        match self {
            RangeOp::InvalidateInstruction => Maintenance::InvalidateInstructionLine,
            RangeOp::CleanInvalidateData => Maintenance::CleanInvalidateDataLine,
            RangeOp::CleanData => Maintenance::CleanDataLine,
            RangeOp::InvalidateData => Maintenance::InvalidateDataLine,
        }
    }
}

/// A zero `start` or zero `length` selects the whole cache. Otherwise one line operation
/// per 32 bytes from `start & !31` while the address is signed-less than the wrapped
/// `start + length`, at least once. Returns stock's r0: the address after the last line,
/// CLIDR for the set/way walks, 0 for the whole invalidates.
#[inline(always)]
pub fn range<C: Cp15>(c: &C, op: RangeOp, start: u32, length: u32) -> u32 {
    if start == 0 || length == 0 {
        return match op {
            RangeOp::CleanInvalidateData => set_way(c, Maintenance::CleanInvalidateDataSetWay),
            RangeOp::CleanData => set_way(c, Maintenance::CleanDataSetWay),
            RangeOp::InvalidateData => {
                c.maintain(Maintenance::InvalidateDataAll, 0);
                0
            }
            RangeOp::InvalidateInstruction => {
                c.maintain(Maintenance::InvalidateInstructionAll, 0);
                0
            }
        };
    }
    let end = start.wrapping_add(length);
    let mut line = start & !31;
    loop {
        c.maintain(op.line(), line);
        line = line.wrapping_add(32);
        if (line as i32) >= (end as i32) {
            return line;
        }
    }
}

/// Stock `0x4272c..0x427b4`: every data or unified level below LoC, sets outer and ways
/// inner, both descending. Returns CLIDR.
#[inline(always)]
fn set_way<C: Cp15>(c: &C, op: Maintenance) -> u32 {
    let clidr = c.read_clidr();
    let loc2 = (clidr & 0x0700_0000) >> 23;
    if loc2 == 0 {
        return clidr;
    }
    let mut level2 = 0u32;
    loop {
        let kind = (clidr >> (level2 + (level2 >> 1))) & 7;
        if kind >= 2 {
            c.write_csselr(level2);
            c.isb();
            let ccsidr = c.read_ccsidr();
            let set_shift = (ccsidr & 7) + 4;
            let ways = (ccsidr >> 3) & 0x3ff;
            let way_shift = ways.leading_zeros();
            let mut set = (ccsidr >> 13) & 0x7fff;
            loop {
                let mut way = ways;
                loop {
                    let operand = level2
                        | shift_left_register(way, way_shift)
                        | shift_left_register(set, set_shift);
                    c.maintain(op, operand);
                    if way == 0 {
                        break;
                    }
                    way -= 1;
                }
                if set == 0 {
                    break;
                }
                set -= 1;
            }
        }
        level2 += 2;
        if (loc2 as i32) <= (level2 as i32) {
            return clidr;
        }
    }
}
