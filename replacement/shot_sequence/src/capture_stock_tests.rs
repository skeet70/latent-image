use crate::boundary::ClientRef;
use crate::capture_boundary::{CaptureOptions, SettingResult, SlotEntry, SlotSelection};
use crate::capture_stock;
use std::collections::BTreeMap;

fn put(block: &mut [u8], offset: usize, value: u32) { block[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }

#[test]
fn capture_block_fields() {
    let mut block = [0u8; capture_stock::CAPTURE_BLOCK_BYTES];
    for (offset, value) in [(0x4, 0x40u32), (0x8, 3), (0xc, 0x1000), (0x10, 0x1800), (0x14, 0x2000), (0x38, 0x3000)] {
        put(&mut block, offset, value);
    }
    let parameters = capture_stock::capture_parameters(&block).unwrap();
    assert_eq!(parameters.options, CaptureOptions(0x40));
    assert!(parameters.options.statistics_setup());
    assert!(parameters.reduced());
    assert_eq!(
        (parameters.sources, parameters.profile, parameters.slots, parameters.selection),
        (ClientRef(0x1000), ClientRef(0x1800), ClientRef(0x2000), ClientRef(0x3000))
    );
    assert_eq!(capture_stock::capture_parameters(&block[..59]), None);
}

#[test]
fn the_selection_bytes_are_the_requested_slot_then_the_count() {
    assert_eq!(capture_stock::slot_selection([2, 3]), SlotSelection { requested: 2, count: 3 });
}

#[test]
fn a_setting_command_reads_its_words_and_the_client_values_they_name() {
    let mut block = [0u8; capture_stock::SETTING_BLOCK_BYTES];
    for (offset, value) in [(0x4, 4u32), (0x8, 8), (0xc, 12), (0x10, 16), (0x18, 0x500), (0x1c, 0x600), (0x20, 32), (0x24, 0x700)] {
        put(&mut block, offset, value);
    }
    let parameters = capture_stock::setting_parameters(&block).unwrap();
    let mut memory = BTreeMap::new();
    for (index, byte) in [1u8, 2, 3, 4, 5, 6].into_iter().enumerate() { memory.insert(0x500 + index as u32, byte); }
    memory.insert(0x604, 0x99);
    for (index, byte) in 0xdead_beefu32.to_le_bytes().into_iter().enumerate() { memory.insert(0x704 + index as u32, byte); }
    let overrides = capture_stock::setting_overrides(&parameters, |at, out| {
        for (index, byte) in out.iter_mut().enumerate() { *byte = memory.get(&(at.0 + index as u32)).copied().unwrap_or(0); }
    });
    assert_eq!(overrides.words, [4, 12, 16, 8, 32]);
    assert_eq!(overrides.window, [1, 2, 3, 4, 5, 6]);
    assert_eq!(overrides.option, 0x99);
    assert_eq!(overrides.selector, 0xdead_beef);
}

#[test]
fn a_setting_result_skips_the_status_word() {
    let mut payload = [0u8; 24];
    for index in 0..6 { put(&mut payload, 4 * index, index as u32 + 10); }
    assert_eq!(capture_stock::setting_result(&payload), Some(SettingResult([11, 12, 13, 14])));
    assert_eq!(capture_stock::setting_result(&payload[..19]), None);
}

#[test]
fn slot_entry_fields() {
    let mut entry = SlotEntry([0; SlotEntry::BYTES]);
    entry.0[0x8..0xa].copy_from_slice(&(-3i16).to_le_bytes());
    entry.0[0xc..0xe].copy_from_slice(&5i16.to_le_bytes());
    entry.0[0xe..0x10].copy_from_slice(&6i16.to_le_bytes());
    put(&mut entry.0, 0x24, 1);
    put(&mut entry.0, 0x28, 0x77);
    put(&mut entry.0, 0xa0, 1);
    entry.set_step(-9);
    entry.set_residual(12);
    assert_eq!((entry.offset(), entry.trims(), entry.step(), entry.residual()), (-3, [5, 6], -9, 12));
    assert_eq!((entry.rate_active(), entry.rate_profile(), entry.primary_subjects()), (1, 0x77, true));
    assert_eq!(&entry.0[0x30..0x32], &(-9i16).to_le_bytes());
    assert_eq!(&entry.0[0x2e..0x30], &12i16.to_le_bytes());
}
