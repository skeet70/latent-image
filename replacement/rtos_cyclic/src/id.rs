use crate::layout::{CyclicSlot, Domain, KernelContext};
use crate::machine::Machine;
use crate::status::CyclicStatus;

/// The raw r0 word; only the entry decodes it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct EncodedId(pub i32);

impl EncodedId {
    pub const fn of(class: u16, index: u16) -> Self {
        Self(((class as i32) << 16) | index as i32)
    }

    pub const fn class(self) -> i32 {
        self.0 >> 16
    }

    pub const fn index(self) -> i32 {
        (self.0 << 16) >> 16
    }
}

/// Create and delete ignore the class half: `uxth(id)` indexes the entry domain's own table.
#[inline(always)]
pub fn own_cyclic_slot(id: EncodedId) -> Result<CyclicSlot, CyclicStatus> {
    let index = (id.0 as u32) & 0xffff;
    CyclicSlot::new(index, CyclicSlot::COUNT).ok_or(CyclicStatus::InvalidIndex)
}

#[inline(always)]
pub fn target_domain(entry: Domain, class: i32) -> Result<Domain, CyclicStatus> {
    match class {
        0 => Ok(entry),
        1 => Ok(Domain::A),
        2 => Ok(Domain::B),
        _ => Err(CyclicStatus::InvalidClass),
    }
}

#[inline(always)]
pub fn target_context<M: Machine>(
    m: &M,
    entry: Domain,
    class: i32,
) -> Result<KernelContext, CyclicStatus> {
    match class {
        0 => Ok(m.current_context()),
        _ => {
            let domain = target_domain(entry, class)?;
            Ok(m.context(domain))
        }
    }
}
