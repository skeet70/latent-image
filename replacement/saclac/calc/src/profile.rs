//! Reads of the bound profile record and the selection policies the library derives from it.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::field::{RecordField, RecordRead, SharedField, SharedRead, Value};
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::{MeasurementSelection, Memory, ProfileRecords, SharedHeaderByte, StockEntries};

/// Kinds for which the refinement pass maps style 1 to 0 and style 3 to 2 (0xf61610).
const REMAPPED_KINDS: [u32; 7] = [5, 11, 12, 34, 37, 40, 48];

/// The profile records as the library reads them: the typed view and the stock header accessors
/// other tasks share.
pub struct Profile<'a, M, E> {
    records: &'a ProfileRecords<M>,
    entries: &'a mut E,
}

impl<'a, M: Memory, E: StockEntries> Profile<'a, M, E> {
    pub fn new(records: &'a ProfileRecords<M>, entries: &'a mut E) -> Self { Self { records, entries } }

    /// The field in the record the handle reads. A registry whose selection names neither record
    /// reads zero, where stock reads past the records.
    pub fn read<T: Value, W>(&self, handle: RecordHandle, field: &RecordField<T, RecordRead, W>) -> T {
        self.records.read(handle, field).unwrap_or_else(|_| T::decode(&[0; 2048][..T::BYTES]))
    }

    /// The field of the block beside the records; a refused read reads zero.
    pub fn read_shared<T: Value, W>(&self, handle: RecordHandle, field: &SharedField<T, SharedRead, W>) -> T {
        self.records.read_shared(handle, field).unwrap_or_else(|_| T::decode(&[0; 2048][..T::BYTES]))
    }

    pub fn header(&mut self, handle: RecordHandle, byte: SharedHeaderByte) -> u8 { self.entries.header_byte(handle, byte) }

    pub fn measurement_selection(&mut self, handle: RecordHandle) -> MeasurementSelection { self.entries.measurement_selection(handle) }

    /// The lens designation beside the records (0x758c68).
    pub fn lens_designation(&mut self, handle: RecordHandle) -> [u8; 28] { self.entries.lens_designation(handle) }

    /// Whether the record withholds its selection policies on a refinement pass.
    fn withheld(&mut self, handle: RecordHandle, refinement_pass: bool) -> bool {
        refinement_pass && self.header(handle, SharedHeaderByte::At63) != 0
    }

    /// The selection policy kind (0xf612dc).
    pub fn selection_kind(&mut self, handle: RecordHandle, refinement_pass: bool) -> u32 {
        let kind = self.header(handle, SharedHeaderByte::At31);
        if self.withheld(handle, refinement_pass) { 0 } else { u32::from(kind) }
    }

    /// The selection policy flags (0xf61584).
    pub fn selection_flags(&mut self, handle: RecordHandle, refinement_pass: bool) -> u16 {
        let flags = self.read(handle, &group_00::AT_002E);
        if self.withheld(handle, refinement_pass) { 0 } else { flags }
    }

    /// The selection policy's auxiliary byte (0xf6171c).
    pub fn selection_auxiliary(&mut self, handle: RecordHandle, refinement_pass: bool) -> u8 {
        let auxiliary = self.header(handle, SharedHeaderByte::At46);
        if self.withheld(handle, refinement_pass) { 0 } else { auxiliary }
    }

    /// The selection style (0xf615bc). A record with header byte 0x63 set narrows the style for
    /// some kinds and replaces it with 6 when the flags carry bit 1.
    pub fn selection_style(&mut self, handle: RecordHandle, refinement_pass: bool) -> u8 {
        let kind = self.selection_kind(handle, refinement_pass);
        let style = self.read(handle, &group_00::AT_003F);
        let flags = self.selection_flags(handle, refinement_pass);
        if self.header(handle, SharedHeaderByte::At63) == 0 {
            return style;
        }
        let style = match style {
            1 if REMAPPED_KINDS.contains(&kind) => 0,
            3 if REMAPPED_KINDS.contains(&kind) => 2,
            other => other,
        };
        if flags & 2 != 0 { 6 } else { style }
    }

    /// 0xf616e8.
    pub fn style_option(&mut self, handle: RecordHandle, option: u32) -> u8 {
        if self.selection_style(handle, false) == 7 || option == 1 { 0 } else { 4 }
    }
}
