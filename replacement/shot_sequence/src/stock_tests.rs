use crate::boundary::{ClientRef, LensReference};
use crate::capture_boundary::ReferencePair;
use crate::ports::{AbortStatus, HalfPressMode, HalfPressNotifications};
use crate::subjects::SubjectId;
use crate::{stock, summary};
use crate::Outcome;
use xm1_saclac_controllers::children::ShotControl;
use xm1_saclac_interface::CompletionStatus;

#[test]
fn control_codes_round_trip_and_unknown_codes_are_refused() {
    for control in [ShotControl::StartS1, ShotControl::AdvanceS1, ShotControl::InitializeCapture, ShotControl::StartLiveSequence] {
        assert_eq!(stock::control(stock::control_code(control)), Some(control));
    }
    assert_eq!(stock::control_code(ShotControl::InitializeCapture), 0x103);
    assert_eq!(stock::control(0xed), None);
}

#[test]
fn outcome_words_are_the_ones_parents_test() {
    assert_eq!(stock::outcome_word(Outcome::Complete), 1);
    assert_eq!(stock::outcome_word(Outcome::Pending), 2);
    assert_eq!(stock::outcome_word(Outcome::Unhandled), 3);
    assert_eq!(stock::outcome_word(Outcome::Progress), 21);
    assert_eq!(stock::outcome_word(Outcome::Aborted), 0);
}

#[test]
fn half_press_block_fields() {
    let mut block = [0u8; 64];
    block[4..8].copy_from_slice(&5u32.to_le_bytes());
    block[0x10..0x14].copy_from_slice(&0b1000u32.to_le_bytes());
    assert_eq!(stock::half_press_mode(&block), Some(HalfPressMode(5)));
    assert_eq!(stock::half_press_notifications(&block), Some(HalfPressNotifications { frame_released: false, relative_lens_done: true }));
    block[0x10] = 0b0100;
    assert_eq!(stock::half_press_notifications(&block), Some(HalfPressNotifications { frame_released: true, relative_lens_done: false }));
    assert_eq!(stock::half_press_mode(&block[..7]), None);
    assert_eq!(stock::half_press_notifications(&block[..0x13]), None);
}

#[test]
fn the_client_reference_pair_takes_its_reference_from_the_block_end() {
    let mut block = [0u8; 64];
    block[0x38..0x3c].copy_from_slice(&0x1111u32.to_le_bytes());
    block[0x3c..0x40].copy_from_slice(&0x2222u32.to_le_bytes());
    let parameters = stock::half_press_parameters(&block).unwrap();
    assert_eq!(parameters.client_reference, ReferencePair { reference: 0x2222, detail: 0x1111 });
}

#[test]
fn only_statuses_13_and_14_abort() {
    assert_eq!(stock::abort_status(13), Some(AbortStatus(CompletionStatus(13))));
    assert_eq!(stock::abort_status(14), Some(AbortStatus(CompletionStatus(14))));
    for word in [0, 12, 15, u32::MAX] { assert_eq!(stock::abort_status(word), None); }
}

#[test]
fn half_press_mode_selects_synchronization_and_skips() {
    assert!(HalfPressMode(0).synchronizes_lens() && HalfPressMode(0).synchronizes_adjustment());
    assert!(!HalfPressMode(6).synchronizes_adjustment());
    assert!(HalfPressMode(1).skips_relative_lens_move());
    assert!(HalfPressMode(5).two_pass_exposure());
}

fn put(bytes: &mut [u8], offset: usize, value: u32) { bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }

#[test]
fn half_press_parameters_name_their_client_addresses_or_none() {
    let mut block = [0u8; 64];
    for (offset, value) in [(4, 2), (0xc, 9), (0x10, 6), (0x18, 0x100), (0x1c, 0x55), (0x24, 0x400), (0x2c, 0x300), (0x30, 40)] { put(&mut block, offset, value); }
    let parameters = stock::half_press_parameters(&block).unwrap();
    assert_eq!(parameters.mode, HalfPressMode(2));
    assert_eq!(parameters.option, 9);
    assert_eq!(parameters.area_count, Some(ClientRef(0x100)));
    assert_eq!(parameters.areas, None);
    assert_eq!(parameters.lens_target, Some(ClientRef(0x400)));
    assert_eq!(parameters.frame_slot, Some(ClientRef(0x300)));
    assert_eq!(parameters.lens_reference, LensReference(40));
    assert!(parameters.mask.publishes_area_selection() && !parameters.mask.publishes_areas());
    assert!(stock::half_press_parameters(&block[..63]).is_none());
}

#[test]
fn record_layouts_keep_their_unnamed_bytes() {
    let mut bytes = [0u8; 48];
    for (index, byte) in bytes.iter_mut().enumerate() { *byte = index as u8; }
    assert_eq!(stock::lens_snapshot_bytes(&stock::lens_snapshot(&bytes)), bytes);
    let snapshot = stock::lens_snapshot(&bytes);
    assert_eq!(snapshot.current.reference, LensReference(u32::from_le_bytes([4, 5, 6, 7])));
    assert_eq!(snapshot.held.position, i16::from_le_bytes([16, 17]));

    let mut result = [0u8; 132];
    put(&mut result, 0x80, 128);
    result[0x70] = 0xaa;
    let mut parsed = stock::focus_result(&result);
    assert_eq!(parsed.extent, 128);
    parsed.refined = 3;
    let written = stock::focus_result_bytes(&parsed);
    assert_eq!(written[0x68], 3);
    assert_eq!(written[0x70], 0xaa);
}

#[test]
fn a_subject_is_identified_by_its_word_at_0x5c() {
    let mut bytes = [0u8; stock::SUBJECT_BYTES];
    put(&mut bytes, 0x5c, 77);
    bytes[0x60] = 1;
    let parsed = stock::subject(&bytes);
    assert_eq!(parsed.id, SubjectId(77));
    assert_eq!(stock::subject_bytes(&parsed), bytes);
    let mut list = [0u8; stock::SUBJECT_LIST_BYTES];
    put(&mut list, 0, 2);
    put(&mut list, 4 + stock::SUBJECT_BYTES + 0x5c, 9);
    let parsed = stock::subject_list(&list);
    assert_eq!((parsed.count, parsed.entries[1].id), (2, SubjectId(9)));
    assert_eq!(stock::subject_list_bytes(&parsed), list);
}

#[test]
fn area_statistics_and_their_summary_words() {
    let mut descriptor = [0u8; 12];
    put(&mut descriptor, 0, 1);
    descriptor[4] = 3;
    let mut entries = [0u8; 5 * stock::AREA_ENTRY_BYTES];
    put(&mut entries, 0, 11);
    put(&mut entries, 4 + 4 * (3 * 8 + 3), 40);
    put(&mut entries, 0x104 + 4 * (3 * 8 + 3), 8);
    put(&mut entries, 0x224, 0x1234);
    entries[0x208..0x20a].copy_from_slice(&(-2i16).to_le_bytes());
    let statistics = stock::area_statistics(&descriptor, &entries);
    assert_eq!((statistics.present, statistics.layout, statistics.entries[0].id), (1, 3, 11));
    assert_eq!((statistics.entries[0].values[3][3], statistics.entries[0].weights[3][3]), (40, 8));
    assert_eq!(statistics.entries[0].fields[0], 0x1234);
    assert_eq!(statistics.entries[0].fields[3], (-2i32) as u32);

    let mut words = std::collections::BTreeMap::new();
    stock::summary_words(&summary::summarize(&statistics), |offset, value| { words.insert(offset, value); });
    let centre = 3 * 7 + 3;
    assert_eq!(words[&0], 0x5354_5253);
    assert_eq!(words[&0x99c], 0x4c41_5253);
    assert_eq!(words[&8], 3);
    assert_eq!(words[&0xc], 11);
    assert_eq!(words[&(0x20 + 5 * 4 * centre)], 40);
    assert_eq!(words[&(0x3f4 + 5 * 4 * centre)], 8);
    assert_eq!(words[&0x7c8], 40);
    assert_eq!(words[&(0x7c8 + 0x64)], 8);
    assert_eq!(words[&0x890], 0x1234);
    assert!(!words.contains_key(&(0x20 + 4)), "absent entries write no cells");
    assert_eq!(words[&0x998], 0);
}
