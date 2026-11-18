#![forbid(unsafe_code)]

//! X-M1 1.50 layout of the handle registry, the records and the shared block.

use crate::field::ChangeBank;
use crate::handle::{AccessGroup, Slot};

/// 0x75e118 clears 0x38c bytes from 0x915d38.
pub const REGISTRY_BYTES: usize = 0x38c;
/// The record stride at 0x74e5f4 and 418 other sites; 0x75e1cc clears 0x42b0 bytes, two records.
pub const RECORD_BYTES: usize = 0x2158;
/// 0x756700 clears 0x9d4 bytes from 0x91904c.
pub const SHARED_BYTES: usize = 0x9d4;

/// The selected-record word: 0x75e1ec reads it, 0x75e20c stores only 0 or 1 there.
pub const SELECTED: u16 = 0x8;

const SLOTS: u16 = 0xc;
const SLOT_STRIDE: u16 = 0x38;

const fn slot_base(slot: Slot) -> u16 { SLOTS + SLOT_STRIDE * slot.index() as u16 }

/// The serial the slot was opened with, all ones while free (0x75e3d0, 0x75e4a8); 0x75aab4
/// compares it with the handle's.
pub const fn slot_serial(slot: Slot) -> u16 { slot_base(slot) }

/// The slot's own record, 0 or 1; any other value defers to `SELECTED` (0x75e3d4, 0x75e618).
pub const fn slot_record(slot: Slot) -> u16 { slot_base(slot) + 0x8 }

/// One grant byte per group (0x75e3e0 copies 43): bit 0 read (0x75e5c8), bit 1 write (0x75e564).
pub const fn slot_grant(slot: Slot, group: AccessGroup) -> u16 { slot_base(slot) + 0xc + group.index() as u16 }

pub const GRANT_READ: u8 = 1;
pub const GRANT_WRITE: u8 = 2;

/// The two 63-bit change masks a field writer ORs its bit into (0x75ac1c).
pub const fn change_mask(bank: ChangeBank) -> u16 {
    match bank {
        ChangeBank::First => 0x80,
        ChangeBank::Second => 0x88,
    }
}

/// The group the change-tracked header is read under (0x74e020, 0x759250).
pub const HEADER_GROUP: AccessGroup = AccessGroup::at(0);
