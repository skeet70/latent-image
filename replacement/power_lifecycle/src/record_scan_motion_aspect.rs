use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionAspectMode { Normalize, Preserve }

impl MotionAspectMode {
    pub const fn from_selector(selector: u32) -> Self {
        if selector == 0 { Self::Normalize } else { Self::Preserve }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionAspect {
    pub numerator: u16,
    pub denominator: u16,
}

impl MotionAspect {
    pub fn normalize(self, mode: MotionAspectMode) -> Self {
        if mode == MotionAspectMode::Preserve { return self; }
        match (self.numerator, self.denominator) {
            (9, 16) | (1, 1) => Self { numerator: 2, denominator: 3 },
            _ => self,
        }
    }
}

pub fn normalize_aspect<M: GuestAddressSpace>(memory: &mut M, mode: MotionAspectMode, address: GuestAddress) -> Result<(), MemoryError> {
    if mode == MotionAspectMode::Preserve { return Ok(()); }
    let denominator_address = address.checked_add(2)?;
    let denominator = memory.read_u16(denominator_address)?;
    if denominator == 3 { return Ok(()); }
    if denominator != 1 && denominator != 4 && denominator != 16 { return Ok(()); }
    let numerator = memory.read_u16(address)?;
    let aspect = MotionAspect { numerator, denominator };
    let normalized = aspect.normalize(mode);
    if normalized != aspect {
        memory.write_u16(denominator_address, normalized.denominator)?;
        memory.write_u16(address, normalized.numerator)?;
    }
    Ok(())
}

pub fn helper_fa60f4<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, mode: MotionAspectMode, aspect: GuestAddress) -> Result<(), MemoryError> {
    normalize_aspect(context, mode, aspect)
}
