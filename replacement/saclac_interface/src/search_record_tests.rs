use crate::search::{ResultAddress, S1DriveRequest, ScanEvaluation, SearchRecord};
use crate::stock::search::{evaluation_address, s1_drive_request, search_record, SEARCH_RECORD_BYTES};

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn half(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

#[test]
fn a_search_record_lays_out_every_field_and_names_the_statistics_banks() {
    let record = SearchRecord {
        first: true,
        scale: [0x1111, 0x2222],
        settled: true,
        level_matched: true,
        selection: 0x0303_0303,
        mode: 0x0404_0404,
        ratio: [0x0505_0505, 0x0606_0606],
        range: [-2, 0x0777],
        range_offset: -9,
        extent: [0x0808_0808, 0x0909_0909],
        window: [0x0a0a_0a0a, 0x0b0b_0b0b],
        frame_mode: 1,
        scan: true,
        profile_class: 0x0c0c_0c0c,
        resource: 0x0d0d_0d0d,
        scan_evaluation: ScanEvaluation::Scanned,
        size: [0x0e0e_0e0e, 0x0f0f_0f0f],
        timing: 0x1010_1010,
        decade: 0x1212,
        correction: 0x1313_1313,
        limit: -300,
    };
    let bytes = search_record(&record);
    assert_eq!(bytes.len(), SEARCH_RECORD_BYTES);
    assert_eq!(word(&bytes, 0x00), 1);
    assert_eq!([half(&bytes, 0x04), half(&bytes, 0x06)], [0x1111, 0x2222]);
    assert_eq!([word(&bytes, 0x08), word(&bytes, 0x0c)], [1, 1]);
    assert_eq!([word(&bytes, 0x10), word(&bytes, 0x14)], [0x0303_0303, 0x0404_0404]);
    assert_eq!([word(&bytes, 0x18), word(&bytes, 0x1c)], [0x0505_0505, 0x0606_0606]);
    assert_eq!([half(&bytes, 0x20) as i16, half(&bytes, 0x22) as i16, half(&bytes, 0x24) as i16], [-2, 0x0777, -9]);
    assert_eq!([word(&bytes, 0x28), word(&bytes, 0x2c)], [0x0808_0808, 0x0909_0909]);
    assert_eq!([word(&bytes, 0x30), word(&bytes, 0x34)], [0x0a0a_0a0a, 0x0b0b_0b0b]);
    assert_eq!([word(&bytes, 0x38), word(&bytes, 0x3c), word(&bytes, 0x40)], [0x5ffe_be8c, 0x5ffe_c08c, 0x5ffe_e518]);
    assert_eq!([word(&bytes, 0x44), word(&bytes, 0x48)], [1, 1]);
    assert_eq!([word(&bytes, 0x4c), word(&bytes, 0x50), word(&bytes, 0x54)], [0x0c0c_0c0c, 0x0d0d_0d0d, 2]);
    assert_eq!(word(&bytes, 0x58), 0x5ffe_c1b4);
    assert_eq!([word(&bytes, 0x5c), word(&bytes, 0x60), word(&bytes, 0x64)], [0x0e0e_0e0e, 0x0f0f_0f0f, 0x1000]);
    assert_eq!(word(&bytes, 0x68), 0x1010_1010);
    assert_eq!([half(&bytes, 0x6c), half(&bytes, 0x6e)], [0, 0x1212]);
    assert_eq!(word(&bytes, 0x70), 0x1313_1313);
    assert_eq!([half(&bytes, 0x74) as i16, half(&bytes, 0x76) as i16], [-300, 0]);
}

#[test]
fn scan_evaluation_takes_the_code_calc_reads() {
    let code = |scan_evaluation| word(&search_record(&SearchRecord { scan_evaluation, ..SearchRecord::default() }), 0x54);
    assert_eq!([code(ScanEvaluation::Plain), code(ScanEvaluation::Scan), code(ScanEvaluation::Scanned)], [0, 1, 2]);
}

#[test]
fn an_evaluation_completion_names_the_result_address_after_the_status() {
    let payload = [0xfe, 0xff, 0xff, 0xff, 0x40, 0x0f, 0x6c, 0x01];
    assert_eq!(evaluation_address(&payload), Some(ResultAddress(0x016c_0f40)));
    assert_eq!(evaluation_address(&payload[..7]), None);
}

#[test]
fn an_s1_drive_request_lays_out_travel_rate_motion_speed_and_mode() {
    let request = S1DriveRequest { travel: -5, rate: 0x0a0b_0c0d, moves: true, speed: 2, lens_mode: 3 };
    assert_eq!(
        s1_drive_request(&request),
        [0xfb, 0xff, 0, 0, 0x0d, 0x0c, 0x0b, 0x0a, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]
    );
}
