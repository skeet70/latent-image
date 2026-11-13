#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

//! saclac's view of the two profile records, the registry whose handles select between them
//! and gate access to their 43 field groups, and the shared block beside them. It replaces the
//! image-4 accessors only task 43 reaches; the accessors other tasks share stay stock behind
//! `StockEntries`.

pub mod entries;
pub mod field;
pub mod handle;
pub mod image;
pub mod memory;
pub mod view;
#[allow(unsafe_code)]
pub mod stock;

pub use entries::{Grants, OpenMode, SharedHeaderByte, StockEntries};
pub use field::{ChangeBank, ChangeBit, ChangeSet, Index, RecordArray, RecordField, SharedArray, SharedField};
pub use handle::{Access, AccessGroup, DeriveRefused, Handle, Scope, Serial, Slot, Tag};
pub use image::RecordImage;
pub use memory::{Bytes, Location, Memory, RecordIndex};
pub use view::{ProfileRecords, Refusal};

#[cfg(test)]
mod tests;
