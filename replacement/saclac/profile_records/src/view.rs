#![forbid(unsafe_code)]

//! Gated, record-selecting reads and writes of the profile records and the shared block.

use xm1_saclac_interface::RecordHandle;

use crate::field::{ChangeBank, ChangeBit, ChangeSet, Index, RecordArray, RecordField, RecordRead, RecordWrite, SharedArray, SharedField, SharedRead, SharedWrite, Unread, Value};
use crate::handle::{AccessGroup, Handle};
use crate::image::RecordImage;
use crate::memory::{Block, Location, Memory, RecordIndex};
use crate::stock::{self, layout, record};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The handle's slot does not grant the access the accessor checks.
    NotGranted,
    /// The registry's selected-record word names neither record.
    SelectionOutOfRange(u32),
}

pub struct ProfileRecords<M> {
    memory: M,
}

impl<M: Memory> ProfileRecords<M> {
    pub const fn new(memory: M) -> Self { Self { memory } }

    pub fn memory(&self) -> &M { &self.memory }

    pub fn into_memory(self) -> M { self.memory }

    /// The record every handle without its own grant reads.
    pub fn selected(&self) -> Result<RecordIndex, Refusal> {
        let word = self.memory.read_u32(Location::new(Block::Registry, layout::SELECTED));
        match word {
            0 => Ok(RecordIndex::First),
            1 => Ok(RecordIndex::Second),
            other => Err(Refusal::SelectionOutOfRange(other)),
        }
    }

    /// The handle's own record, or the selected one when its slot names neither.
    fn slot_record(&self, handle: Handle) -> Result<RecordIndex, Refusal> {
        match self.memory.read_u32(Location::new(Block::Registry, layout::slot_record(handle.slot))) {
            0 => Ok(RecordIndex::First),
            1 => Ok(RecordIndex::Second),
            _ => self.selected(),
        }
    }

    fn is_open(&self, handle: Handle) -> bool {
        self.memory.read_u32(Location::new(Block::Registry, layout::slot_serial(handle.slot))) == u32::from(handle.serial.get())
    }

    fn grant(&self, handle: Handle, group: AccessGroup) -> u8 {
        self.memory.read_u8(Location::new(Block::Registry, layout::slot_grant(handle.slot, group)))
    }

    pub fn may_read(&self, handle: RecordHandle, group: AccessGroup) -> bool {
        stock::handle(handle).is_some_and(|handle| {
            self.is_open(handle) && handle.scope.admits(group) && self.grant(handle, group) & layout::GRANT_READ != 0
        })
    }

    pub fn may_write(&self, handle: RecordHandle, group: AccessGroup) -> bool {
        stock::handle(handle).is_some_and(|handle| {
            self.is_open(handle)
                && handle.scope.admits(group)
                && handle.writable
                && self.grant(handle, group) & layout::GRANT_WRITE != 0
        })
    }

    /// The record a read under `group` lands in.
    pub fn read_record(&self, handle: RecordHandle, group: AccessGroup) -> Result<RecordIndex, Refusal> {
        match stock::handle(handle) {
            Some(decoded) if self.may_read(handle, group) => self.slot_record(decoded),
            _ => self.selected(),
        }
    }

    fn write_record(&self, handle: RecordHandle, gate: RecordWrite) -> Result<RecordIndex, Refusal> {
        let granted = match gate {
            RecordWrite::Selected(group) => return self.read_record(handle, group),
            RecordWrite::Granted(group) => self.may_write(handle, group),
            RecordWrite::GrantedAll(groups) => groups.iter().all(|&group| self.may_write(handle, group)),
        };
        match stock::handle(handle) {
            Some(decoded) if granted => self.slot_record(decoded),
            _ => Err(Refusal::NotGranted),
        }
    }

    pub fn read<T: Value, W>(&self, handle: RecordHandle, field: &RecordField<T, RecordRead, W>) -> Result<T, Refusal> {
        let record = self.read_record(handle, field.read.0)?;
        Ok(T::load(&self.memory, Location::new(Block::Record(record), field.offset)))
    }

    /// Stores the value, then sets the field's change bit in the same record.
    pub fn write<T: Value, R>(&mut self, handle: RecordHandle, field: &RecordField<T, R, RecordWrite>, value: T) -> Result<(), Refusal> {
        let record = self.write_record(handle, field.write)?;
        value.store(&mut self.memory, Location::new(Block::Record(record), field.offset));
        if let Some(change) = field.change {
            self.mark_changed(record, change);
        }
        Ok(())
    }

    pub fn read_element<T: Value, const N: usize, W: Copy>(&self, handle: RecordHandle, array: &RecordArray<T, N, RecordRead, W>, index: Index<N>) -> Result<T, Refusal> {
        self.read(handle, &array.at(index))
    }

    pub fn write_element<T: Value, const N: usize, R: Copy>(&mut self, handle: RecordHandle, array: &RecordArray<T, N, R, RecordWrite>, index: Index<N>, value: T) -> Result<(), Refusal> {
        self.write(handle, &array.at(index), value)
    }

    pub fn read_shared<T: Value, W>(&self, handle: RecordHandle, field: &SharedField<T, SharedRead, W>) -> Result<T, Refusal> {
        match field.read {
            SharedRead::Requires(group) if !self.may_write(handle, group) => Err(Refusal::NotGranted),
            _ => Ok(T::load(&self.memory, Location::new(Block::Shared, field.offset))),
        }
    }

    pub fn write_shared<T: Value, R>(&mut self, handle: RecordHandle, field: &SharedField<T, R, SharedWrite>, value: T) -> Result<(), Refusal> {
        match field.write {
            SharedWrite::Granted(group) if !self.may_write(handle, group) => Err(Refusal::NotGranted),
            _ => {
                value.store(&mut self.memory, Location::new(Block::Shared, field.offset));
                Ok(())
            }
        }
    }

    pub fn read_shared_element<T: Value, const N: usize, W: Copy>(&self, handle: RecordHandle, array: &SharedArray<T, N, SharedRead, W>, index: Index<N>) -> Result<T, Refusal> {
        self.read_shared(handle, &array.at(index))
    }

    pub fn write_shared_element<T: Value, const N: usize, R: Copy>(&mut self, handle: RecordHandle, array: &SharedArray<T, N, R, SharedWrite>, index: Index<N>, value: T) -> Result<(), Refusal> {
        self.write_shared(handle, &array.at(index), value)
    }

    fn change_mask(&self, record: RecordIndex, bank: ChangeBank) -> u64 {
        let at = layout::change_mask(bank);
        let low = self.memory.read_u32(Location::new(Block::Record(record), at));
        let high = self.memory.read_u32(Location::new(Block::Record(record), at + 4));
        u64::from(high) << 32 | u64::from(low)
    }

    fn store_change_mask(&mut self, record: RecordIndex, bank: ChangeBank, mask: u64) {
        let at = layout::change_mask(bank);
        self.memory.write_u32(Location::new(Block::Record(record), at), mask as u32);
        self.memory.write_u32(Location::new(Block::Record(record), at + 4), (mask >> 32) as u32);
    }

    fn mark_changed(&mut self, record: RecordIndex, change: ChangeBit) {
        let mask = self.change_mask(record, change.bank) | change.mask();
        self.store_change_mask(record, change.bank, mask);
    }

    /// Whether every bit of `changes` is set in the record a header read would read.
    pub fn has_changes(&self, handle: RecordHandle, changes: ChangeSet) -> Result<bool, Refusal> {
        let record = self.read_record(handle, layout::HEADER_GROUP)?;
        Ok(self.change_mask(record, changes.bank) & changes.bits == changes.bits)
    }

    /// Clears both change masks of the record a header read would read, then writes each field
    /// `staged` marks changed, in stock's order. Each write is gated alone, and a refused one is
    /// skipped.
    pub fn apply_changes(&mut self, handle: RecordHandle, staged: &RecordImage) -> Result<(), Refusal> {
        let record = self.read_record(handle, layout::HEADER_GROUP)?;
        self.store_change_mask(record, ChangeBank::First, 0);
        self.store_change_mask(record, ChangeBank::Second, 0);
        for tracked in &record::CHANGE_TRACKED {
            let applied = match tracked {
                record::Tracked::U8(field) => self.apply(handle, staged, field),
                record::Tracked::U16(field) => self.apply(handle, staged, field),
                record::Tracked::I8(field) => self.apply(handle, staged, field),
                record::Tracked::I16(field) => self.apply(handle, staged, field),
                record::Tracked::U32(field) => self.apply(handle, staged, field),
                record::Tracked::Bytes4(field) => self.apply(handle, staged, field),
                record::Tracked::Bytes6(field) => self.apply(handle, staged, field),
                record::Tracked::Bytes8(field) => self.apply(handle, staged, field),
                record::Tracked::Selection(field) => self.apply(handle, staged, field),
                record::Tracked::IntervalTracking(field) => self.apply(handle, staged, field),
            };
            match applied {
                Err(Refusal::NotGranted) | Ok(()) => {}
                Err(fault) => return Err(fault),
            }
        }
        Ok(())
    }

    fn apply<T: Value>(&mut self, handle: RecordHandle, staged: &RecordImage, field: &RecordField<T, Unread, RecordWrite>) -> Result<(), Refusal> {
        match field.change {
            Some(change) if staged.changed(change) => self.write(handle, field, staged.get(field)),
            _ => Ok(()),
        }
    }

    /// 0x752714: the halfword when the flag byte is 1, otherwise the word.
    pub fn chosen_word(&self, handle: RecordHandle) -> Result<u32, Refusal> {
        if self.read(handle, &record::CHOICE_FLAG)? == 1 {
            self.read(handle, &record::CHOICE_HALFWORD).map(u32::from)
        } else {
            self.read(handle, &record::CHOICE_WORD)
        }
    }
}
