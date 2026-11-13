use crate::pipeline::*;
use crate::stock::pipeline::*;

#[test]
fn word_results_round_trip_through_their_payloads() {
    assert_eq!(raw_check(&raw_check_payload(RawCheck(0x0102_0304))), Some(RawCheck(0x0102_0304)));
    assert_eq!(tone_status(&tone_status_payload(ToneStatus(7))), Some(ToneStatus(7)));
    let progress = ToneProgress { status: 0x8000_0001, level: 0xfedc };
    assert_eq!(tone_progress_payload(progress), [1, 0, 0, 0x80, 0xdc, 0xfe]);
    assert_eq!(tone_progress(&tone_progress_payload(progress)), Some(progress));
    assert_eq!(tone_progress(&[0; 5]), None);
    assert_eq!(raw_check(&[0; 3]), None);
}

#[test]
fn calculation_results_take_their_leading_bytes() {
    let payload: [u8; 120] = core::array::from_fn(|index| index as u8);
    assert_eq!(raw_calculation(&payload).unwrap().0, payload[..20]);
    assert_eq!(tone_calculation(&payload).unwrap().0, payload[..108]);
    assert_eq!(gradation_calculation(&payload).unwrap().0, payload[..96]);
    assert_eq!(tone_calculation(&payload[..107]), None);
    assert_eq!(gradation_calculation(&payload[..95]), None);
    assert_eq!(raw_calculation(&payload[..19]), None);
}

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }
fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) }

const SUBJECT: SubjectGeometry = SubjectGeometry { profile: 0x11, state: 0x22, horizontal: 0x33, vertical: 0x44, extent: 0x55 };
const AREA: CaptureArea = CaptureArea { reference: 0x66, areas: [0x71, 0x72, 0x73], area: 0x77 };

#[test]
fn the_frame_requests_place_subject_and_area_where_the_image_task_reads_them() {
    let raw = raw_frame_request(&RawFrameRequest { subject: SUBJECT, area: AREA, image_buffer: 0x88, raw_word: 0x99 });
    assert_eq!((word(&raw, 0), word(&raw, 4), half(&raw, 8), half(&raw, 10), word(&raw, 12)), (0x11, 0x22, 0x33, 0x44, 0x55));
    assert_eq!((word(&raw, 0x10), word(&raw, 0x14), word(&raw, 0x1c), word(&raw, 0x20)), (0x66, 0x71, 0x73, 0x77));
    assert_eq!(raw[0x24..0x28], [100, 100, 16, 0]);
    assert_eq!((word(&raw, 0x28), word(&raw, 0x2c)), (0x88, 0x99));

    let frame = frame_request(&FrameRequest { subject: SUBJECT, kind: 4, area: AREA, image_buffer: 0x88, sampling: 0xaa, mode: 3 });
    assert_eq!((half(&frame, 12), half(&frame, 14), word(&frame, 0x10), word(&frame, 0x14), word(&frame, 0x24)), (4, 0, 0x55, 0x66, 0x77));
    assert_eq!((word(&frame, 0x28), word(&frame, 0x2c), frame[0x30], frame[0x31..].iter().all(|byte| *byte == 0)), (0x88, 0xaa, 3, true));
}

#[test]
fn the_scan_request_lays_out_each_window_kind() {
    let mut request = ScanRequest { start: 1, positions: [10, 20, 30], span: 5, selection_word: 0x40, index_base: 6, correction: 0xfff0, ..ScanRequest::default() };
    request.windows[0] = 0xab;
    let none = scan_request(&request);
    assert_eq!((word(&none, 0), half(&none, 4), half(&none, 6), half(&none, 8), word(&none, 12), half(&none, 0x10), half(&none, 0x12)), (1, 10, 20, 30, 0x40, 5, 6));
    assert!(none[0x1c..0x40].iter().all(|byte| *byte == 0));
    assert_eq!((none[0x44], half(&none, 0x74)), (0xab, 0xfff0));

    request.window = ScanWindow::Centred { centre: [1, 2], first: [3, 4], second: [5, 6], held: true };
    let centred = scan_request(&request);
    assert_eq!([0x1c, 0x20, 0x24, 0x28, 0x2c, 0x30, 0x34, 0x38, 0x3c].map(|offset| word(&centred, offset)), [1, 2, 5, 6, 3, 4, 1, 1, 1]);

    request.window = ScanWindow::Area([7; 36]);
    assert!(scan_request(&request)[0x1c..0x40].iter().all(|byte| *byte == 7));
}

#[test]
fn the_calculation_requests_carry_levels_window_and_gate() {
    let tone = tone_request(&ToneRequest { levels: [-2; TONE_LEVELS], offset: 9, window: [1, 2, 3, 4, 5, 6] });
    assert_eq!((half(&tone, 0), half(&tone, 0x20), half(&tone, 0x22), word(&tone, 0x24), word(&tone, 0x28), word(&tone, 0x3c)), (0xfffe, 0xfffe, 9, 1, 1, 6));

    let mut merged = [0x5a; TONE_CALCULATION_BYTES];
    merged[4..8].copy_from_slice(&0u32.to_le_bytes());
    let request = GradationRequest { tone: ToneCalculation(merged), gated: false, sampled: true, exposure: 0x1234 };
    let open = gradation_request(&request);
    assert_eq!((word(&open, 4), open[0x68], word(&open, 0x6c), half(&open, 0x70), half(&open, 0x72)), (0, 0x5a, 1, 0x1234, 0));
    assert_eq!(word(&gradation_request(&GradationRequest { gated: true, ..request }), 4), 1);
}

#[test]
fn the_statistics_request_fills_its_five_blocks_and_arguments() {
    let setup = StatisticsSetup {
        mode: 3,
        weighted: true,
        selection: [1, 2, 3],
        area: [4; 16],
        levels: [5, 6, 7, 8],
        sampling: StatisticsSampling { capture: [10, 11, 12, 13, 14, 15, 16], lens: [20, 21], halves: [30, 31, 32, 33], sample: 40, flags: [41, 42, 43] },
        capture: StatisticsCapture { words: [50, 51, 52, 53], flags: [60, 61, 62, 63, 64], layout: 70 },
    };
    let blocks = statistics_blocks(&setup);
    assert_eq!((blocks.format, blocks.kind, word(&blocks.selection, 0xc), word(&blocks.levels, 0xc)), (3, 2, 1, 8));
    assert_eq!([0, 0xc, 0x10, 0x14, 0x1c, 0x20, 0x24].map(|offset| word(&blocks.sampling, offset)), [10, 13, 1, 14, 16, 20, 21]);
    assert_eq!((half(&blocks.sampling, 0x28), half(&blocks.sampling, 0x2e), word(&blocks.sampling, 0x30), blocks.sampling[0x36]), (30, 33, 40, 43));
    assert_eq!((word(&blocks.capture, 0xc), blocks.capture[0x1d], blocks.capture[0x21], half(&blocks.capture, 0x34)), (53, 60, 64, 70));
    assert_eq!(statistics_blocks(&StatisticsSetup { weighted: false, ..setup }).kind, 0);
}
