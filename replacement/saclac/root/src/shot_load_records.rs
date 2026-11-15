#![forbid(unsafe_code)]

use xm1_saclac_interface::{stock::StockCallbackWord, RecordHandle};
use xm1_saclac_profile_records::{field::Index, stock::{record as r, shared as s}, Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::LensSnapshot, capture_boundary::{ReferencePair, SlotEntry, SlotHandle}, record_load::{AreaStarts, LoadRecords, ModeFiveBlock, PairSlot, ResumeSource, StoredArea}, stock};
use crate::shot_services::Libraries;

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> LoadRecords for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn record_lens_snapshot(&mut self, record: RecordHandle) -> LensSnapshot {
        stock::lens_snapshot(&self.services.borrow().shared_bytes(record, &s::group_17::LENS_SNAPSHOT))
    }
    fn store_record_lens_snapshot(&mut self, record: RecordHandle, snapshot: &LensSnapshot) {
        let _ = self.services.borrow_mut().records.write_shared(record, &s::group_17::LENS_SNAPSHOT, stock::lens_snapshot_bytes(snapshot));
    }
    fn lens_reference(&mut self, record: RecordHandle) -> u32 { self.services.borrow().field(record, &r::group_01::AT_00BC) }
    fn resume_offset(&mut self, record: RecordHandle, source: ResumeSource) -> i16 {
        let services = self.services.borrow();
        match source {
            ResumeSource::Profile => services.field(record, &r::group_00::AT_001C),
            ResumeSource::Stored => services.shared(record, &s::group_41::AT_0972),
        }
    }
    fn store_resume_offset(&mut self, record: RecordHandle, offset: i16) {
        let _ = self.services.borrow_mut().records.write_shared(record, &s::group_41::AT_0972, offset);
    }
    fn store_reference_pair(&mut self, record: RecordHandle, slot: PairSlot, pair: ReferencePair) {
        let mut bytes = [0; 8];
        bytes[..4].copy_from_slice(&pair.reference.to_le_bytes());
        bytes[4..].copy_from_slice(&pair.detail.to_le_bytes());
        let _ = self.services.borrow_mut().records.write_shared_element(record, &s::group_41::AT_0974, Index::new(usize::from(slot.get())).expect("reference pair slot"), bytes);
    }
    fn store_area_starts(&mut self, record: RecordHandle, starts: AreaStarts) {
        let mut services = self.services.borrow_mut();
        let _ = services.records.write_shared(record, &s::group_41::AT_096C, starts.stored);
        let _ = services.records.write_shared(record, &s::group_41::AT_096E, starts.selected);
    }
    fn mode_five_profile(&mut self, record: RecordHandle) -> [i16; 3] {
        let services = self.services.borrow();
        [&r::group_01::AT_0092, &r::group_01::AT_00B4, &r::group_01::AT_00B6].map(|field| services.field(record, field))
    }
    fn mode_five_block(&mut self, record: RecordHandle) -> ModeFiveBlock {
        ModeFiveBlock(self.services.borrow().shared_bytes(record, &s::group_33::AT_07A0))
    }
    fn store_mode_five_block(&mut self, record: RecordHandle, block: &ModeFiveBlock) {
        let _ = self.services.borrow_mut().records.write_shared(record, &s::group_33::AT_07A0, block.0);
    }
    fn stored_area(&mut self, record: RecordHandle) -> StoredArea {
        let services = self.services.borrow();
        macro_rules! field { ($group:ident::$name:ident) => { services.records.read_element(record, &r::$group::$name, Index::new(0).expect("first area")).unwrap_or_default() }; }
        StoredArea {
            start: field!(group_05::AT_016C_2), reference: field!(group_05::AT_0170),
            selection_class: field!(group_06::AT_01A0), plan_half: field!(group_06::AT_01A4),
            plan_bit_two: field!(group_06::AT_01A8), plan_bit_five: field!(group_06::AT_01AC),
            halves: [field!(group_05::AT_0176), field!(group_05::AT_0178), field!(group_05::AT_017A), field!(group_05::AT_017C), field!(group_05::AT_0186)],
        }
    }
    fn record_slot(&mut self, record: RecordHandle, slot: u8) -> SlotEntry {
        SlotEntry(Index::new(usize::from(slot)).and_then(|slot| self.services.borrow().records.read_element(record, &r::group_05::AT_016C_168, slot).ok()).unwrap_or([0; SlotEntry::BYTES]))
    }
    fn store_client_slot(&mut self, target: SlotHandle, entry: &SlotEntry) {
        write_client(&mut self.services.borrow().machine.clone(), target.0, &entry.0);
    }
    fn auxiliary_hook(&mut self, record: RecordHandle) -> Option<StockCallbackWord> {
        let word = self.services.borrow().shared(record, &s::group_20::AT_0158);
        (word != 0).then_some(StockCallbackWord(word))
    }
}

fn write_client(machine: &mut impl Machine, mut address: u32, mut bytes: &[u8]) {
    while !bytes.is_empty() && address & 3 != 0 {
        machine.write_u8(address, bytes[0]);
        address = address.wrapping_add(1);
        bytes = &bytes[1..];
    }
    let mut words = bytes.chunks_exact(4);
    for word in &mut words {
        machine.write_u32(address, u32::from_le_bytes(word.try_into().expect("client word")));
        address = address.wrapping_add(4);
    }
    for &byte in words.remainder() {
        machine.write_u8(address, byte);
        address = address.wrapping_add(1);
    }
}
