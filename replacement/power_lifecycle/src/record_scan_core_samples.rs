//! X-M1 1.50 sample reduction, source pin 55436fc85c0560dd252b0410af6f2427fe198cd3.

use super::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplePair {
    pub primary: u32,
    pub secondary: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RankLimit(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleCell {
    row: usize,
    column: usize,
}

impl SampleCell {
    pub fn from_doubled(row: u32, column: u32) -> Option<Self> {
        let row = (row >> 1) as usize;
        let column = (column >> 1) as usize;
        if row > 6 || column > 6 { None } else { Some(Self { row, column }) }
    }
}

pub fn select_cell(samples: &[u32; 128], cell: SampleCell) -> SamplePair {
    let index = cell.row + cell.column * 8;
    SamplePair { primary: samples[index + 64], secondary: samples[index] }
}

#[derive(Default)]
struct Accumulator {
    primary: u64,
    secondary: u64,
    count: u32,
}

impl Accumulator {
    fn add(&mut self, pair: SamplePair, weight: u8) {
        self.primary += u64::from(pair.primary.wrapping_mul(u32::from(weight)));
        self.secondary += u64::from(pair.secondary.wrapping_mul(u32::from(weight)));
        self.count += u32::from(weight);
    }
    fn finish(self) -> SamplePair {
        let divisor = u64::from(self.count.max(1));
        SamplePair { primary: (self.primary / divisor) as u32, secondary: (self.secondary / divisor) as u32 }
    }
}

pub fn reduce_samples(samples: &[u32; 128], ranks: &[u8; 64], limit: RankLimit, weights: Option<&[u8; 64]>) -> SamplePair {
    let mut total = Accumulator::default();
    for index in 0..64 {
        if u32::from(ranks[index]) <= limit.0 {
            total.add(SamplePair { primary: samples[index + 64], secondary: samples[index] }, weights.map_or(1, |values| values[index]));
        }
    }
    total.finish()
}

pub fn helper_f88dcc<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    samples: GuestAddress,
    mode: u32,
    limit: RankLimit,
    cell: GuestAddress,
    weights: Option<GuestAddress>,
    primary: GuestAddress,
    secondary: GuestAddress,
) -> Result<i32, MemoryError> {
    if samples.get() == 0 || primary.get() == 0 || secondary.get() == 0 { return Ok(0x84); }
    if mode == 2 || mode == 5 {
        if cell.get() == 0 { return Ok(0x84); }
        let row = context.read_u32(cell)? >> 1;
        if row > 6 { return Ok(0x84); }
        let column = context.read_u32(cell.checked_add(4)?)? >> 1;
        if column > 6 { return Ok(0x84); }
        let index = row + column * 8;
        let value = context.read_u32(samples.checked_add((index + 64) * 4)?)?;
        context.write_u32(primary, value)?;
        let value = context.read_u32(samples.checked_add(index * 4)?)?;
        context.write_u32(secondary, value)?;
        return Ok(0);
    }
    let mut total = Accumulator::default();
    for index in 0..64 {
        let rank = context.read_u8(GuestAddress::new(0x00f8_9074 + index))?;
        if u32::from(rank) <= limit.0 {
            let weight = match weights { None => 1, Some(base) => context.read_u8(base.checked_add(index)?)? };
            let first = context.read_u32(samples.checked_add((index + 64) * 4)?)?;
            let second = context.read_u32(samples.checked_add(index * 4)?)?;
            total.add(SamplePair { primary: first, secondary: second }, weight);
        }
    }
    let pair = total.finish();
    context.write_u32(primary, pair.primary)?;
    context.write_u32(secondary, pair.secondary)?;
    Ok(0)
}
