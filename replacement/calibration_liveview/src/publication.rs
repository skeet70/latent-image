use crate::Width;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterOffset(pub(crate) usize);
impl ParameterOffset { pub const fn byte_offset(self) -> usize { self.0 } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterBits(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HalfwordParameter(pub u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WhiteBalanceMode(pub i16);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirtyCell { General, Profile, First, Second, Auxiliary }
impl DirtyCell {
    pub const fn offset(self) -> usize { match self { Self::General => 3, Self::Profile => 4, Self::First => 8, Self::Second => 9, Self::Auxiliary => 13 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyBits(pub u8);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Descriptor { First, Second, Third, Fourth }
impl Descriptor {
    pub const fn offset(self) -> usize { match self { Self::First => 0x100, Self::Second => 0x300, Self::Third => 0x500, Self::Fourth => 0x6b8 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessingLane { First, Second }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxiliaryMode(u32);
impl AuxiliaryMode {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1);
    pub const fn parse(raw: u32) -> Self { Self(raw) }
}

pub trait Publication {
    fn input(&self, offset: ParameterOffset, width: Width) -> ParameterBits;
    fn current(&self, offset: ParameterOffset, width: Width) -> ParameterBits;
    fn store(&mut self, offset: ParameterOffset, width: Width, value: ParameterBits);
    fn gate(&self) -> bool;
    fn dirty(&self, cell: DirtyCell) -> DirtyBits;
    fn raise(&mut self, cell: DirtyCell, bits: DirtyBits);
    fn descriptor(&self, descriptor: Descriptor) -> bool;
    fn lane(&self, lane: ProcessingLane) -> bool;
    fn white_balance_mode(&self) -> WhiteBalanceMode;
    fn white_balance_copy(&mut self);
    fn white_balance_convert(&mut self, mode: WhiteBalanceMode);
    fn white_balance_publish(&mut self);
    fn halfword_publish(&mut self, value: HalfwordParameter);
    fn auxiliary_mode(&self) -> AuxiliaryMode;
    fn auxiliary_value(&self) -> ParameterBits;
    fn store_auxiliary_value(&mut self, value: ParameterBits);
    fn lock(&mut self);
    fn rebuild(&mut self);
    fn unlock(&mut self);
}

fn general(p: &mut impl Publication) {
    #[cfg(control_wrong_dirty)]
    let bits = DirtyBits(2);
    #[cfg(not(control_wrong_dirty))]
    let bits = DirtyBits(1);
    if p.gate() { p.raise(DirtyCell::General, bits); }
}
fn changed(p: &mut impl Publication, start: usize, widths: &[Width]) -> bool {
    let mut offset = start;
    let mut values = [ParameterBits(0); 3];
    let mut unequal = false;
    for (index, width) in widths.iter().enumerate() {
        #[cfg(control_wrong_field)]
        let source = if offset == 0x5c { 0x60 } else { offset };
        #[cfg(not(control_wrong_field))]
        let source = offset;
        values[index] = p.input(ParameterOffset(source), *width);
        unequal |= p.current(ParameterOffset(offset), *width) != p.input(ParameterOffset(offset), *width);
        offset += width.bytes();
    }
    if unequal {
        #[cfg(control_dirty_before_store)]
        if start == 0x5c { general(p); }
        offset = start;
        for (width, value) in widths.iter().zip(values) {
            #[cfg(control_alias_reread)]
            let value = { let _ = value; p.input(ParameterOffset(offset), *width) };
            p.store(ParameterOffset(offset), *width, value);
            offset += width.bytes();
        }
    }
    unequal
}
fn profile(p: &mut impl Publication, lane: ProcessingLane, bit: u8) {
    if (p.descriptor(Descriptor::First) || p.descriptor(Descriptor::Second)) && p.lane(lane) {
        p.raise(DirtyCell::Profile, DirtyBits(bit));
    }
}
fn auxiliary(p: &mut impl Publication) {
    let active = p.input(ParameterOffset(0x8c), Width::Word);
    #[cfg(control_wrong_request)]
    let stored_request = ParameterBits(active.0 ^ 2);
    #[cfg(not(control_wrong_request))]
    let stored_request = active;
    p.store(ParameterOffset(0x8c), Width::Word, stored_request);
    let kind = p.input(ParameterOffset(0x98), Width::Word);
    p.store(ParameterOffset(0x98), Width::Word, kind);
    if active.0 == 1 && kind.0 == 0 {
        let mut update = false;
        if changed(p, 0x90, &[Width::Word]) {
            let value = p.current(ParameterOffset(0x90), Width::Word);
            if value != p.auxiliary_value() {
                if p.auxiliary_mode() == AuxiliaryMode::ZERO { p.store_auxiliary_value(value); }
                update = true;
            }
        }
        update |= changed(p, 0x94, &[Width::Word]);
        let mismatch = p.auxiliary_mode() == AuxiliaryMode::ONE && p.auxiliary_value() != p.current(ParameterOffset(0x90), Width::Word);
        if update || mismatch {
            if p.lane(ProcessingLane::First) && p.dirty(DirtyCell::Auxiliary).0 & 1 != 0 { p.raise(DirtyCell::Profile, DirtyBits(8)); }
            if p.lane(ProcessingLane::Second) && p.dirty(DirtyCell::Auxiliary).0 & 2 != 0 { p.raise(DirtyCell::Profile, DirtyBits(16)); }
        }
    }
    p.store(ParameterOffset(0x8c), Width::Word, ParameterBits(0));
}

pub fn publish(p: &mut impl Publication) {
    if changed(p, 0, &[Width::Word, Width::Word, Width::Word]) {
        profile(p, ProcessingLane::First, 2);
        profile(p, ProcessingLane::Second, 4);
        if p.descriptor(Descriptor::Third) { p.raise(DirtyCell::First, DirtyBits(0x86)); }
        if p.descriptor(Descriptor::Fourth) { p.raise(DirtyCell::Second, DirtyBits(1)); }
        general(p);
    }
    changed(p, 0x0c, &[Width::Word]);
    if changed(p, 0x10, &[Width::Word]) { general(p); }
    changed(p, 0xc4, &[Width::Halfword]);
    for offset in [0x14, 0x18] { if changed(p, offset, &[Width::Word]) { general(p); } }
    if changed(p, 0x1c, &[Width::Word, Width::Word]) { general(p); }
    if changed(p, 0x2c, &[Width::Word, Width::Word]) { profile(p, ProcessingLane::First, 2); }
    if changed(p, 0x34, &[Width::Word, Width::Word]) { profile(p, ProcessingLane::Second, 4); }
    let mode = p.white_balance_mode();
    if changed(p, 0x3c, &[Width::Word, Width::Halfword]) {
        p.white_balance_copy();
        p.white_balance_convert(mode);
        general(p);
        p.white_balance_publish();
    }
    for offset in [0x44, 0x74, 0x48] { if changed(p, offset, &[Width::Word]) { general(p); } }
    for offset in [0x4c, 0x50] { changed(p, offset, &[Width::Word]); }
    if changed(p, 0x5c, &[Width::Word]) {
        #[cfg(not(control_dirty_before_store))]
        general(p);
    }
    changed(p, 0x60, &[Width::Word]);
    for offset in [0x64, 0x68, 0x6c] { if changed(p, offset, &[Width::Word]) { general(p); } }
    if changed(p, 0x70, &[Width::Byte]) { general(p); }
    if changed(p, 0x78, &[Width::Halfword]) { general(p); }
    auxiliary(p);
    for offset in [0x9c, 0x54] { if changed(p, offset, &[Width::Word]) { general(p); } }
    changed(p, 0xa0, &[Width::Byte]);
    if changed(p, 0xa2, &[Width::Halfword]) {
        general(p);
        let value = HalfwordParameter(p.current(ParameterOffset(0xa2), Width::Halfword).0 as u16);
        p.halfword_publish(value);
    }
    for offset in [0xa4, 0xa8, 0xac, 0xb0] { if changed(p, offset, &[Width::Word]) { general(p); } }
    if changed(p, 0xbc, &[Width::Byte]) { general(p); }
    for offset in [0xb4, 0xb8] { if changed(p, offset, &[Width::Word]) { general(p); } }
    changed(p, 0xc0, &[Width::Word]);
}

pub fn publish_and_rebuild(p: &mut impl Publication) {
    p.lock();
    publish(p);
    p.rebuild();
    p.unlock();
}
