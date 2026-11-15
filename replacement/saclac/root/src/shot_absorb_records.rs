#![forbid(unsafe_code)]

use xm1_saclac_interface::{calculation::{AreaIndex, CellSummary, FocusSummary}, RecordHandle};
use xm1_saclac_profile_records::{field::Index, stock::{record as r, shared as s}, Memory, ProfileRecords, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{absorb::{AbsorbRecords, AreaDetail, AreaStore, AuxiliaryReference}, boundary::MeteringBlock, capture_boundary::{AreaReference, CaptureRecords, RecordField}, stock::absorb::{self as wire, RecordStore}};
use crate::shot_services::Libraries;

fn area_value<R: Memory>(records: &mut ProfileRecords<R>, record: RecordHandle, index: Index<3>, store: RecordStore) {
    macro_rules! put { ($group:ident::$field:ident, $type:ty) => {{ let _ = records.write_element(record, &r::$group::$field, index, store.value as $type); }}; }
    match store.setter {
        0x7529e8 => put!(group_05::AT_016C_2, i16),
        0x752a98 => put!(group_05::AT_0170, u32),
        0x752b48 => put!(group_05::AT_0174, i16),
        0x752bf8 => put!(group_05::AT_0176, i16),
        0x752ca8 => put!(group_05::AT_0178, i16),
        0x752d58 => put!(group_05::AT_017A, i16),
        0x752f68 => put!(group_05::AT_017C, i16),
        0x753018 => put!(group_05::AT_017E, i16),
        0x7530c8 => put!(group_05::AT_0180, i16),
        0x753598 => put!(group_05::AT_0182, i16),
        0x753228 => put!(group_05::AT_0184, i16),
        0x753178 => put!(group_05::AT_0186, i16),
        0x753388 => put!(group_05::AT_0188, u32),
        0x753438 => put!(group_05::AT_018C, u32),
        0x7534e8 => put!(group_05::AT_0190, u32),
        0x753648 => put!(group_05::AT_0194, u32),
        0x7532d8 => put!(group_05::AT_0198, u8),
        0x7536f8 => put!(group_06::AT_01A0, u32),
        0x7537a8 => put!(group_06::AT_01A4, i16),
        0x753858 => put!(group_06::AT_01A8, u32),
        0x753908 => put!(group_06::AT_01AC, u32),
        0x7539b8 => put!(group_07::AT_01B0, u32),
        0x753a68 => put!(group_07::AT_01B4, u32),
        0x753b18 => put!(group_07::AT_01B8, u32),
        0x753bc8 => put!(group_07::AT_01BC, u32),
        0x753f38 => put!(group_08::AT_01F0, i16),
        0x753fe8 => put!(group_08::AT_01F2, i16),
        0x7541f8 => put!(group_08::AT_01FC, u32),
        0x7544b8 => put!(group_10::AT_0208, u32),
        0x754568 => put!(group_10::AT_020C, u32),
        0x7542a8 => put!(group_10::AT_0210, u32),
        _ => panic!("unknown absorb area setter"),
    }
}

fn shared_value<R: Memory>(records: &mut ProfileRecords<R>, record: RecordHandle, store: RecordStore) {
    macro_rules! put { ($group:ident::$field:ident, $type:ty) => {{ let _ = records.write_shared(record, &s::$group::$field, store.value as $type); }}; }
    match store.setter {
        0x756fb8 => put!(group_26::AT_01FC, i16),
        0x757004 => put!(group_26::AT_0200, u32),
        0x7570f0 => put!(group_26::AT_0204, u16),
        0x757050 => put!(group_26::AT_0206, i16),
        0x7570a0 => put!(group_26::AT_0208, i16),
        0x757140 => put!(group_26::AT_020C, i16),
        0x757190 => put!(group_26::AT_0212, u16),
        0x7571e0 => put!(group_26::AT_0216, i16),
        0x75722c => put!(group_26::AT_0218, u32),
        0x757274 => put!(group_26::AT_0230, u32),
        0x757360 => put!(group_26::AT_0234, u16),
        0x7573ac => put!(group_26::AT_0236, u8),
        0x7572c0 => put!(group_26::AT_0238, i16),
        0x7573f4 => put!(group_26::AT_023C, u32),
        0x757534 => put!(group_27::AT_0248, i16),
        0x757b98 => put!(group_27::AT_0708, u32),
        0x757be4 => put!(group_27::AT_070C, i16),
        0x757c34 => put!(group_27::AT_070E, i16),
        0x757cd4 => put!(group_27::AT_0712, i16),
        0x757e10 => put!(group_27::AT_071C, u32),
        0x757ea0 => put!(group_27::AT_0724, u32),
        0x757fc8 => put!(group_27::AT_0730, u32),
        _ => panic!("unknown absorb shared setter"),
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> AbsorbRecords for Libraries<'_, M, R, E, P, I, T, A, Q>
where Self: CaptureRecords {
    fn store_area(&mut self, record: RecordHandle, index: AreaIndex, store: &AreaStore) {
        let index = Index::new(usize::from(index.get())).expect("absorb area index");
        let mut services = self.services.borrow_mut();
        for value in wire::area_stores(store) { area_value(&mut services.records, record, index, value); }
    }
    fn store_area_detail(&mut self, record: RecordHandle, index: AreaIndex, detail: &AreaDetail) {
        let index = Index::new(usize::from(index.get())).expect("absorb area index");
        let mut services = self.services.borrow_mut();
        for value in wire::detail_stores(detail).into_iter().flatten() { area_value(&mut services.records, record, index, value); }
    }
    fn store_auxiliary(&mut self, record: RecordHandle, index: AreaIndex, auxiliary: &AuxiliaryReference) {
        let index = Index::new(usize::from(index.get())).expect("absorb area index");
        let mut services = self.services.borrow_mut();
        for value in wire::auxiliary_stores(auxiliary) { area_value(&mut services.records, record, index, value); }
    }
    fn store_cell(&mut self, record: RecordHandle, cell: &CellSummary, weight: u32) {
        let mut services = self.services.borrow_mut();
        for value in wire::cell_stores(cell, weight) { shared_value(&mut services.records, record, value); }
    }
    fn store_focus_summary(&mut self, record: RecordHandle, summary: &FocusSummary, level: u32) {
        let mut services = self.services.borrow_mut();
        for value in wire::focus_summary_stores(summary, level) { shared_value(&mut services.records, record, value); }
    }
    fn area_lens_reference(&mut self, record: RecordHandle, index: u8) -> u32 {
        Index::new(usize::from(index)).and_then(|index| self.services.borrow().records.read_element(record, &r::group_05::AT_0170, index).ok()).unwrap_or(0)
    }
    fn extended_areas(&mut self, record: RecordHandle) -> bool { self.services.borrow().field(record, &r::group_01::AT_00C4) != 0 }
    fn selection_source(&mut self, record: RecordHandle) -> u32 {
        // f1d6f0/700: the full word at+44 of the record's48-byte lens snapshot.
        let bytes = self.services.borrow().shared_bytes(record, &s::group_17::LENS_SNAPSHOT);
        u32::from_le_bytes(bytes[44..48].try_into().expect("auxiliary selection source"))
    }
    fn metering(&mut self, record: RecordHandle) -> MeteringBlock { MeteringBlock(self.services.borrow().shared_bytes(record, &s::group_18::AT_00E0)) }
    fn store_metering(&mut self, record: RecordHandle, block: &MeteringBlock) {
        let _ = self.services.borrow_mut().records.write_shared(record, &s::group_18::AT_00E0, block.0);
    }
    fn value(&mut self, record: RecordHandle, field: RecordField) -> u32 { CaptureRecords::field(self, record, field) }
    fn area_references(&mut self, record: RecordHandle) -> AreaReference { CaptureRecords::area_reference(self, record) }
}
