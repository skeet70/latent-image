use crate::area_measurement::*;
use crate::calc::{AreaGeometry, SharedAddress};
use crate::stock::area_measurement as stock;

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }

fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) }

#[test]
fn the_measurement_request_carries_the_record_values_selection_and_areas_where_imgdc_reads_them() {
    let request = MeasurementRequest {
        subject_source: 1,
        subjects_locked: 2,
        layout: [3, 4],
        secondary_mode: 5,
        secondary_option: 6,
        secondary_latch: 7,
        profile_class: 8,
        capture_offset: 9,
        frame_words: [10, 11],
        selection: CaptureSelection { selection: 12, first: -13, second: 14, auxiliary: 15 },
        slot_offset: -16,
        slot_rate: 17,
        subject_area: 18,
        lists: [true, false],
        list_details: [19, 0],
        area_count: 2,
        shift: -3,
    };
    let bytes = stock::measurement_request(&request, SharedAddress(0x016b_76fc));
    let words = [(0x00, 1), (0x04, 2), (0x0c, 5), (0x10, 6), (0x14, 7), (0x1c, 9), (0x20, 10), (0x24, 11), (0x28, 0), (0x2c, 1), (0x30, 1)];
    for (offset, value) in words { assert_eq!(word(&bytes, offset), value, "word at {offset:#x}"); }
    let more = [(0x34, 12), (0x40, 15), (0x44, 17), (0x4c, 18), (0x50, 1), (0x54, 0), (0x58, 19), (0x5c, 0), (0x60, 2), (0x64, 0x016b_76fc)];
    for (offset, value) in more { assert_eq!(word(&bytes, offset), value, "word at {offset:#x}"); }
    let halves = [(0x08, 3), (0x0a, 4), (0x18, 8), (0x38, (-16i16) as u16), (0x3a, (-13i16) as u16), (0x3c, 14), (0x48, 0x50), (0x4a, 0x10), (0x68, (-3i16) as u16)];
    for (offset, value) in halves { assert_eq!(half(&bytes, offset), value, "halfword at {offset:#x}"); }
}

#[test]
fn completions_decode_the_fields_the_secondary_job_takes() {
    assert_eq!(stock::measurement_ready(&[0, 0, 0, 0, 6, 0, 0, 0]), Some(MeasurementReady { source: 6 }));
    assert_eq!(stock::measurement_ready(&[0; 4]), None);

    let mut payload = [0u8; 84];
    payload[0xc..0xe].copy_from_slice(&(-2i16).to_le_bytes());
    payload[0x16..0x18].copy_from_slice(&7i16.to_le_bytes());
    for (index, byte) in payload[0x20..0x38].iter_mut().enumerate() { *byte = index as u8; }
    payload[0x44..0x48].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    let refinement = stock::refinement(&payload).unwrap();
    assert_eq!((refinement.fifth, refinement.slot_offset, refinement.slot_word), (7, -2, 0x1234_5678));
    assert_eq!(refinement.block, core::array::from_fn(|index| index as u8));
    assert_eq!(stock::refinement(&payload[..0x40]), None);

    payload[0x3c] = 2;
    payload[0x40] = 3;
    payload[0x50] = 1;
    payload[0x4e..0x50].copy_from_slice(&(-500i16).to_le_bytes());
    let evaluation = stock::evaluation(&payload).unwrap();
    assert_eq!((evaluation.list(), evaluation.list_position(), evaluation.stores_limit(), evaluation.second_limit()), (Some(EvaluatedList::Second), 3, true, -500));
    assert_eq!(evaluation.base(), -2);
}

#[test]
fn the_capture_selection_reads_two_words_around_two_halfwords() {
    let mut bytes = [0u8; 12];
    bytes[0..4].copy_from_slice(&5u32.to_le_bytes());
    bytes[4..6].copy_from_slice(&(-1i16).to_le_bytes());
    bytes[6..8].copy_from_slice(&0x8001u16.to_le_bytes());
    bytes[8..12].copy_from_slice(&9u32.to_le_bytes());
    assert_eq!(stock::capture_selection(&bytes), CaptureSelection { selection: 5, first: -1, second: 0x8001, auxiliary: 9 });
}

#[test]
fn the_geometry_block_marks_each_measured_area_and_keeps_the_areas_where_the_request_points() {
    let mut area = AreaEntry::EMPTY;
    area.set_rectangle([1, 2, 3, 4]);
    area.set_centre([5, 6]);
    area.set_scale(7);
    let geometry = AreaGeometry {
        active: true,
        list: Some(EvaluatedList::First),
        settings: [8, 9, 10],
        count: 2,
        shares: [11, 12, 0],
        scale: 13,
        list_value: 14,
        area_count: 2,
        areas: [area, AreaEntry::EMPTY, AreaEntry::EMPTY],
        accepted: [true, false, false],
        span: 3,
        shift: -1,
        record_value: 15,
        within_limit: true,
        slot_rate: 16,
    };
    let bytes = stock::area_geometry(&geometry);
    assert_eq!((word(&bytes, 0), word(&bytes, 4)), (2, 1));
    assert_eq!(&bytes[0x08..0x0e], &[0, 0, 8, 9, 10, 2]);
    assert_eq!(&bytes[0x10..0x20], &[0xff; 16]);
    assert_eq!(&bytes[0x20..0x28], &[0; 8]);
    assert_eq!((word(&bytes, 0x28), word(&bytes, 0x2c), half(&bytes, 0x34), word(&bytes, 0x38), word(&bytes, 0x3c)), (11, 12, 13, 14, 2));
    assert_eq!(&bytes[stock::AREAS_OFFSET..stock::AREAS_OFFSET + AreaEntry::BYTES], &area.0);
    assert_eq!((word(&bytes, 0x178), word(&bytes, 0x17c)), (1, 0));
    assert_eq!((half(&bytes, 0x184), half(&bytes, 0x186)), (3, 0xffff));
    assert_eq!((word(&bytes, 0x198), word(&bytes, 0x1a0), word(&bytes, 0x1a4)), (15, 1, 16));
}

#[test]
fn an_area_keeps_its_rectangle_centre_and_scale_at_the_record_layout() {
    let mut area = AreaEntry::EMPTY;
    area.set_rectangle([0x10, 0x20, 0x30, 0x40]);
    area.set_centre([0x50, 0x60]);
    area.set_scale(0x7080);
    assert_eq!((word(&area.0, 0), word(&area.0, 0xc), word(&area.0, 0x14), half(&area.0, 0x22)), (0x10, 0x40, 0x60, 0x7080));
    assert_eq!((area.rectangle(), area.centre(), area.scale()), ([0x10, 0x20, 0x30, 0x40], [0x50, 0x60], 0x7080));
}
