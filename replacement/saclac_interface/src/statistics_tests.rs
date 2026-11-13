use crate::calc::SharedAddress;
use crate::statistics::*;
use crate::stock::{statistics::statistics_payload, StockCallbackWord as Cb};
use crate::StatisticsOperation as Op;

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) }
fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap()) }

fn exposure() -> Exposure {
    Exposure {
        settings: [0x101, 0x102, 0x103, 0x104, 0x105, 0x106, 0x107, 0x108],
        window: MeasuredWindow { corrected: [0x201, 0x202, 0x203], level: 0x204, extent: 0x205 },
        adjusted: 0x206,
        linear: 0x207,
        source: Source { word: 0x208, byte: 0x29 },
        class: 0x2a,
        detail: 0x2b,
    }
}

fn capture() -> Capture {
    Capture {
        position: 0x301,
        selection: 0x302,
        settings: [0x303, 0x304],
        retune: 0x305,
        options: [0x31, 0x32, 0x33, 0x34, 0x35],
        spans: [0x306, 0x307],
        dimensions: [0x308, 0x309],
        policy: 0x30a,
    }
}

fn configuration() -> Configuration {
    Configuration {
        setup: Setup { descriptor: 0x1, pair: [0x2, 0x3], tail: SetupTail::Default },
        format: Format { dimensions: [0x11, 0x12], ratio: [0x13, 0x14], value: 0x15, class: 0x16, mode: 0x17 },
        window: [0x21, 0x22, 0x23, 0x24],
        output: [0x31, 0x32, 0x33, 0x34],
        exposure: exposure(),
        capture: capture(),
        notice: Notice { pending: 0x41, results: SharedAddress(0x016c_0728), callback: Cb(0x00f3_a794) },
        flagged: true,
    }
}

/// Checks the blocks 0xf3bd8c, 0xf3bccc and 0xf3bbac all place ahead of their tails.
fn assert_configuration(bytes: &[u8]) {
    assert_eq!((word(bytes, 0), word(bytes, 4), word(bytes, 8), word(bytes, 12)), (0x1, 0x2, 0x3, 0x11de_784a));
    assert_eq!((word(bytes, 16), word(bytes, 20), half(bytes, 24), half(bytes, 26)), (0x11, 0x12, 0x13, 0x14));
    assert_eq!((word(bytes, 28), word(bytes, 32), word(bytes, 36)), (0x15, 0x16, 0x17));
    assert_eq!((word(bytes, 40), word(bytes, 52), word(bytes, 56), word(bytes, 68)), (0x21, 0x24, 0x31, 0x34));
    assert_eq!((word(bytes, 72), word(bytes, 100)), (0x101, 0x108));
    assert_eq!(word(bytes, 104), 0x205);
    assert_eq!([half(bytes, 108), half(bytes, 110), half(bytes, 112)], [0x201, 0x202, 0x203]);
    assert_eq!((half(bytes, 114), half(bytes, 116), half(bytes, 118)), (0x206, 0x204, 0x207));
    assert_eq!((word(bytes, 120), &bytes[124..128]), (0x208, &[0x29, 0x2a, 0x2b, 0][..]));
    assert_eq!((word(bytes, 128), word(bytes, 132), word(bytes, 136), word(bytes, 140), word(bytes, 144)), (0x301, 0x302, 0x303, 0x304, 0x305));
    assert_eq!(&bytes[148..164], &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0x31, 0x32, 0x33, 0x34, 0x35, 0, 0]);
    assert_eq!((word(bytes, 164), word(bytes, 168), word(bytes, 172), word(bytes, 176)), (0x306, 0x307, 0x308, 0x309));
    assert_eq!((half(bytes, 180), half(bytes, 182)), (0x30a, 0));
    assert_eq!((word(bytes, 184), word(bytes, 188), word(bytes, 192)), (0x41, 0x016c_0728, 0x00f3_a794));
}

#[test]
fn start_places_its_completion_flag_and_feature_after_the_blocks() {
    let (operation, message) = statistics_payload(&StatisticsPayload::Start { configuration: configuration(), feature: 4, callback: Cb(0x00f3_aa18) });
    let bytes = message.bytes();
    assert_eq!((operation, bytes.len()), (Op::Request02, 208));
    assert_configuration(bytes);
    assert_eq!((word(bytes, 196), word(bytes, 200), word(bytes, 204)), (0x00f3_aa18, 1, 4));
}

#[test]
fn selected_start_places_the_flag_before_its_completion() {
    let (operation, message) = statistics_payload(&StatisticsPayload::StartSelected { configuration: configuration(), callback: Cb(0x00f3_aa18) });
    let bytes = message.bytes();
    assert_eq!((operation, bytes.len()), (Op::Request06, 204));
    assert_configuration(bytes);
    assert_eq!((word(bytes, 196), word(bytes, 200)), (1, 0x00f3_aa18));
}

#[test]
fn capture_configuration_carries_the_view_word_command09_and_three_completions() {
    let (operation, message) = statistics_payload(&StatisticsPayload::Configure {
        configuration: Configuration { flagged: false, ..configuration() },
        view: 0x77,
        command: CaptureCommand { second: 0x81, third: 0x82, fourth: 0x83 },
        callbacks: [Cb(0x00f3_a9c8), Cb(0x00f3_a9a0), Cb(0x00f3_aa40)],
    });
    let bytes = message.bytes();
    assert_eq!((operation, bytes.len()), (Op::Request08, 248));
    assert_configuration(bytes);
    assert_eq!((word(bytes, 196), word(bytes, 200), word(bytes, 204), word(bytes, 208)), (0, 0, 0x77, 0));
    assert_eq!(&bytes[212..224], &[0; 12]);
    assert_eq!((word(bytes, 224), word(bytes, 228), word(bytes, 232)), (0x81, 0x82, 0x83));
    assert_eq!((word(bytes, 236), word(bytes, 240), word(bytes, 244)), (0x00f3_a9c8, 0x00f3_a9a0, 0x00f3_aa40));
}

#[test]
fn stop_and_release_carry_their_flags_and_completion() {
    let (operation, message) = statistics_payload(&StatisticsPayload::Stop { pause: true, shot_idle: false, callback: Cb(0x00f3_a9f0) });
    assert_eq!((operation, message.bytes()), (Op::Request07, &[1, 0, 0, 0, 0, 0, 0, 0, 0xf0, 0xa9, 0xf3, 0][..]));
    let (operation, message) = statistics_payload(&StatisticsPayload::Release { callback: Cb(0x00f3_a978) });
    assert_eq!((operation, message.bytes()), (Op::Request09, &[0, 0, 0, 0, 0x78, 0xa9, 0xf3, 0][..]));
}

#[test]
fn measured_window_and_source_carry_the_profile_values() {
    let window = MeasuredWindow { corrected: [1, 2, 3], level: 4, extent: 0x0605 };
    let (operation, message) = statistics_payload(&StatisticsPayload::MeasuredWindow(window));
    assert_eq!((operation, message.bytes()), (Op::Request14, &[1, 0, 2, 0, 3, 0, 4, 0, 5, 6, 0, 0][..]));
    let (operation, message) = statistics_payload(&StatisticsPayload::MeasuredSource { source: Source { word: 7, byte: 8 }, position: 9 });
    assert_eq!((operation, message.bytes()), (Op::Request41, &[7, 0, 0, 0, 8, 0, 0, 0, 9, 0, 0, 0][..]));
    let (_, message) = statistics_payload(&StatisticsPayload::MeasuredSource { source: Source { word: 0, byte: 8 }, position: 9 });
    assert_eq!(message.bytes(), &[0; 12]);
}

#[test]
fn adjustment_replaces_the_setup_tail_and_names_both_completions() {
    let setup = Setup { descriptor: 1, pair: [2, 3], tail: SetupTail::Adjustment(0x44) };
    let (operation, message) = statistics_payload(&StatisticsPayload::Adjust { setup, callbacks: [Cb(0x00f3_a950), Cb(0x00f3_a924)] });
    let bytes = message.bytes();
    assert_eq!((operation, bytes.len()), (Op::Request0a, 24));
    assert_eq!([word(bytes, 0), word(bytes, 4), word(bytes, 8), word(bytes, 12), word(bytes, 16), word(bytes, 20)], [1, 2, 3, 0x44, 0x00f3_a950, 0x00f3_a924]);
    let (operation, message) = statistics_payload(&StatisticsPayload::Command13);
    assert_eq!((operation, message.bytes().len()), (Op::Request0c, 0));
}

#[test]
fn prepare_places_the_exposure_after_the_selection_range_and_completion() {
    let (operation, message) = statistics_payload(&StatisticsPayload::Prepare {
        selection: [1, 2, 3], range: [4, 5, 6, 7], exposure: exposure(), callback: Cb(0x00f3_a8ac),
    });
    let bytes = message.bytes();
    assert_eq!((operation, bytes.len()), (Op::Request17, 92));
    assert_eq!([word(bytes, 0), word(bytes, 8), word(bytes, 12), word(bytes, 24), word(bytes, 28), word(bytes, 32)], [1, 3, 4, 7, 0, 0x00f3_a8ac]);
    assert_eq!((word(bytes, 36), word(bytes, 36 + 0x20), half(bytes, 36 + 0x2c), bytes[91]), (0x101, 0x205, 0x204, 0));
}

#[test]
fn single_block_requests_end_with_their_completion() {
    let callback = Cb(0x00f3_a884);
    for (payload, operation, length) in [
        (StatisticsPayload::Output { output: [1, 2, 3, 4], callback }, Op::Request13, 20),
        (StatisticsPayload::Window { window: [1, 2, 3, 4], callback }, Op::Request11, 20),
        (StatisticsPayload::Format { format: configuration().format, callback }, Op::Request10, 28),
        (StatisticsPayload::Exposure { exposure: exposure(), callback }, Op::Request12, 60),
        (StatisticsPayload::Capture { capture: capture(), callback }, Op::Request2c, 60),
    ] {
        let (encoded, message) = statistics_payload(&payload);
        let bytes = message.bytes();
        assert_eq!((encoded, bytes.len(), word(bytes, length - 4)), (operation, length, 0x00f3_a884));
    }
    let (_, message) = statistics_payload(&StatisticsPayload::Capture { capture: capture(), callback });
    assert_eq!((word(message.bytes(), 0x10), &message.bytes()[0x1d..0x22]), (0x305, &[0x31, 0x32, 0x33, 0x34, 0x35][..]));
}

#[test]
fn selection_and_command24_requests() {
    let (operation, message) = statistics_payload(&StatisticsPayload::Selection {
        pending: 1, results: SharedAddress(0x016c_0728), callbacks: [Cb(0x00f3_a7bc), Cb(0x00f3_a794)], flagged: true,
    });
    let bytes = message.bytes();
    assert_eq!(operation, Op::Request2a);
    assert_eq!([word(bytes, 0), word(bytes, 4), word(bytes, 8), word(bytes, 12), word(bytes, 16)], [1, 0x016c_0728, 0x00f3_a7bc, 0x00f3_a794, 1]);
    let command = Command24 { second: 2, third: 3, fourth: 4, fifth: 5, ratio: [6, 7] };
    let (operation, message) = statistics_payload(&StatisticsPayload::Command24 { command, callback: Cb(0x00f3_a768) });
    let bytes = message.bytes();
    assert_eq!(operation, Op::Request2e);
    assert_eq!(core::array::from_fn::<u32, 7, _>(|index| word(bytes, 4 * index)), [0x00f3_a768, 2, 3, 4, 6, 7, 5]);
}

#[test]
fn capture_report_surrounds_the_dimensions_with_the_completion_and_ends_with_the_results() {
    use crate::stock::statistics::{capture_report, CAPTURE_REPORT_BYTES};
    let completion: [u8; 64] = core::array::from_fn(|index| index as u8);
    let results: [u8; RESULTS_BYTES] = core::array::from_fn(|index| (index % 251) as u8);
    let bytes = capture_report(&CaptureReport { ratio: 0x0004_0003, dimensions: [640, 480], completion: &completion, results: &results });
    assert_eq!(bytes.len(), CAPTURE_REPORT_BYTES);
    assert_eq!((word(&bytes, 0), word(&bytes, 4), word(&bytes, 8)), (0x0004_0003, 640, 480));
    assert_eq!(&bytes[12..24], &completion[4..16]);
    assert_eq!((word(&bytes, 24), word(&bytes, 28)), (960, 640));
    assert_eq!(&bytes[32..36], &completion[16..20]);
    assert_eq!(&bytes[36..76], &completion[20..60]);
    assert_eq!(&bytes[76..], &results[..]);
}

#[test]
fn overrides_replace_their_fields_and_keep_the_rest_of_the_record() {
    let mut bytes = [0u8; StatisticsSetting::BYTES];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = index as u8;
    }
    let before = StatisticsSetting::from_bytes(bytes);
    let mut after = before;
    let overrides = SettingOverrides { selector: 0xa1a2_a3a4, words: [1, 2, 3, 4, 5], window: [9; 6], option: 0x77 };
    after.apply(&overrides);
    let replaced = |index: usize| (0x20..0x38).contains(&index) || (0x7c..0x82).contains(&index) || index == 0x8c;
    for index in 0..StatisticsSetting::BYTES {
        if !replaced(index) {
            assert_eq!(after.bytes()[index], before.bytes()[index], "byte {index:#x}");
        }
    }
    assert_eq!(&after.bytes()[0x20..0x28], &[0xa4, 0xa3, 0xa2, 0xa1, 1, 0, 0, 0]);
    assert_eq!(&after.bytes()[0x34..0x38], &5u32.to_le_bytes());
    assert_eq!(&after.bytes()[0x7c..0x82], &[9; 6]);
    assert_eq!(after.bytes()[0x8c], 0x77);
}
