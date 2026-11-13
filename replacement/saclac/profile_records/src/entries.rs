#![forbid(unsafe_code)]

//! Profile-record entries tasks 50, 59 and 104 also call. They stay stock; saclac calls them
//! through this trait, which `stock::Stock` binds to their addresses.

use xm1_saclac_interface::RecordHandle;

use crate::image::RecordImage;
use crate::values::MeasurementSelection;

/// Header bytes whose only reader is a stock accessor other tasks share.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedHeaderByte {
    At31,
    At46,
    At47,
    At61,
    At63,
}

/// Which record a new slot names (0x75aa3c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenMode {
    First,
    Second,
    /// The record not selected, refused while an open slot names it; the open copies the selected
    /// record into it and selects it.
    Copy,
    Selected,
}

/// The grant tables saclac opens slots with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Grants {
    Saclac,
}

pub trait StockEntries {
    /// Read under group 0 with the view's record selection.
    fn measurement_selection(&mut self, handle: RecordHandle) -> MeasurementSelection;
    /// Read under group 0 with the view's record selection.
    fn header_byte(&mut self, handle: RecordHandle, byte: SharedHeaderByte) -> u8;
    /// The bytes of `stock::shared::group_36::AT_0840`, read without a gate.
    fn lens_designation(&mut self, handle: RecordHandle) -> [u8; 28];
    /// Copies `staged`'s header into the handle's record and clears that record's change masks,
    /// when the handle may write group 0.
    fn replace_header(&mut self, handle: RecordHandle, staged: &RecordImage);
    /// The new slot's owner handle, or the all-ones word when no slot is free or `mode` is refused.
    fn open(&mut self, mode: OpenMode, grants: Grants) -> RecordHandle;
    fn close(&mut self, handle: RecordHandle);
    /// Clears the registry, both records and the shared block, and selects the first record.
    fn initialize(&mut self);
}
