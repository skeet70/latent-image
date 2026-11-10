use core::marker::PhantomData;
use crate::{property_table::{PROPERTIES, SLOTS, SPECIALS}, table::{Bits, Kind, SpecialField, Width}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviousUpdate { None, FieldBits, WholeContainer }

pub trait FieldValue: Copy { fn from_bits(value: u32) -> Self; fn bits(self) -> u32; }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropertyValue<const ID: u16>(u32);
impl<const ID: u16> PropertyValue<ID> { pub const fn new(value: u32) -> Self { Self(value) } pub const fn get(self) -> u32 { self.0 } }
impl<const ID: u16> FieldValue for PropertyValue<ID> { fn from_bits(value: u32) -> Self { Self(value) } fn bits(self) -> u32 { self.0 } }

#[derive(Clone, Copy)]
pub struct Field<V> { pub(crate) offset: usize, pub(crate) bits: Bits, marker: PhantomData<V> }
#[derive(Clone, Copy)]
pub struct SlotField<V> { pub(crate) current: Field<V>, pub(crate) record: Field<V> }

#[derive(Clone, Copy)]
pub struct HeaderField<V>(pub(crate) Field<V>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotHeaderField;

impl<V> TryFrom<Field<V>> for HeaderField<V> {
    type Error = NotHeaderField;

    #[inline(always)]
    fn try_from(field: Field<V>) -> Result<Self, Self::Error> {
        let width = match field.bits.width() { Width::Byte => 1, Width::Half => 2 };
        if field.offset + width <= crate::mirror::HEADER_BYTES {
            Ok(Self(field))
        } else {
            Err(NotHeaderField)
        }
    }
}
impl<const ID: u16> Field<PropertyValue<ID>> {
    pub fn property() -> Option<Self> {
        let entry = PROPERTIES.get(usize::from(ID).checked_sub(1)?)?.kind()?;
        let (offset, bits) = match entry {
            Kind::Field(field) => (field.offset.0, field.bits),
            Kind::SlotField(field) => (field.offset.0, field.bits),
            Kind::Special(index) => match SPECIALS.get(index.0 as usize)?.field()? {
                SpecialField::Hooked(field) => (field.offset.0, field.bits),
                SpecialField::ReadOnly(field) => (field.offset.0, field.bits),
                SpecialField::WriteOnlyPair(_) => return None,
            },
            Kind::Unassigned => return None,
        };
        Self::checked(offset as usize, bits, crate::BANK_STRIDE)
    }
}
impl<const ID: u16> SlotField<PropertyValue<ID>> {
    pub fn property() -> Option<Self> {
        let Kind::SlotField(slot) = PROPERTIES.get(usize::from(ID).checked_sub(1)?)?.kind()? else { return None; };
        let record = SLOTS.get(slot.index.0 as usize)?.record()?;
        Some(Self { current: Field::checked(slot.offset.0 as usize, slot.bits, crate::BANK_STRIDE)?, record: Field::checked(record.offset.0 as usize, record.bits, 40)? })
    }
}
impl<V> SlotField<V> {
    #[inline(always)]
    pub fn from_layout(current: Field<V>, offset: crate::table::RecordOffset, bits: Bits) -> Option<Self> {
        Some(Self { current, record: Field::checked(offset.0 as usize, bits, 40)? })
    }
}
impl<V> Field<V> {
    #[inline(always)]
    pub fn from_bank_layout(offset: crate::table::BankOffset, bits: Bits) -> Option<Self> {
        Self::checked(offset.0 as usize, bits, crate::BANK_STRIDE)
    }
    #[inline(always)]
    fn checked(offset: usize, bits: Bits, extent: usize) -> Option<Self> {
        let width = match bits.width() { Width::Byte => 1, Width::Half => 2 };
        if bits.count() == 0 || bits.lsb() + bits.count() > width * 8 || offset > extent.checked_sub(width as usize)? { return None; }
        Some(Self { offset, bits, marker: PhantomData })
    }
    #[inline(never)]
    pub(crate) fn load(&self, source: &[u8], base: usize) -> u32 {
        let offset = base + self.offset;
        match self.bits.width() {
            Width::Byte => u32::from(*source.get(offset).unwrap()),
            Width::Half => u32::from(u16::from_le_bytes([
                *source.get(offset).unwrap(),
                *source.get(offset+1).unwrap(),
            ])),
        }
    }
    #[inline(never)]
    pub(crate) fn store(&self, destination: &mut [u8], base: usize, value: u32) {
        let offset = base + self.offset;
        match self.bits.width() {
            Width::Byte => crate::memory::write_byte(destination.get_mut(offset).unwrap(), value as u8),
            Width::Half => crate::memory::write_halfword(destination, offset, value as u16),
        }
    }
    #[inline(always)]
    pub(crate) fn extract(&self, container: u32) -> u32 { (container >> self.bits.lsb()) & self.bits.mask() }
    #[inline(always)]
    pub(crate) fn insert(&self, container: u32, value: u32) -> u32 {
        let mask = self.bits.mask() << self.bits.lsb();
        (container & !mask) | ((value << self.bits.lsb()) & mask)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteValue<V> { Value(V), Default }
impl<V: FieldValue> WriteValue<V> {
    pub fn from_abi(value: u32) -> Self { if value == 0x1affffff { Self::Default } else { Self::Value(V::from_bits(value)) } }
}
