use super::control::{
    converted_sample_period, exposure_interval, frame_interval, left_reference, line_pair_interval, quantized_shutter,
    readout_window, reference_scale, RateSelection,
};
use super::fake::FakeCapture;
use super::ports::{LoadTestSetting, Reference};
use super::{ProfileHandle, ProfileKind, Rectangle, ShutterLevel, Unsupported};

const PROFILE: ProfileHandle = ProfileHandle(30);

/// 1585 lines of 1192 clocks, 25 frames a second.
const READOUT: Reference = Reference { lines: 1585, line_length: 1192, frame_period: 40_019, left_reference: 62, left: 60 };

fn capture(kind: ProfileKind) -> FakeCapture { FakeCapture::new(kind, READOUT) }

#[test]
fn exposed_frame_interval_is_the_reference_frame_at_the_pixel_clock() {
    let mut ports = capture(ProfileKind::Exposed);
    assert_eq!(frame_interval(&mut ports, PROFILE), Ok(1192 * 1585 / 54));
}

#[test]
fn configured_frame_interval_uses_the_control_state_lines() {
    let mut ports = capture(ProfileKind::Configured);
    ports.configured_lines = 1000;
    assert_eq!(frame_interval(&mut ports, PROFILE), Ok(1192 * 1000 / 54));
}

#[test]
fn tabled_and_rate_limited_profiles_have_no_frame_interval() {
    for kind in [ProfileKind::Tabled, ProfileKind::RateLimited, ProfileKind::Untimed] {
        assert_eq!(frame_interval(&mut capture(kind), PROFILE), Err(Unsupported));
    }
}

#[test]
fn a_frame_limit_load_test_stretches_the_frame_by_period_over_limit_to_even_lines() {
    let mut ports = capture(ProfileKind::Exposed);
    ports.load_test = Some(LoadTestSetting(0x8a));
    ports.set_halfword(0xca, 30);
    // 1585 lines * 40019 / 30000 = 2114.3, rounded and made even.
    assert_eq!(frame_interval(&mut ports, PROFILE), Ok(1192 * 2114 / 54));
}

#[test]
fn other_load_tests_leave_the_frame_alone() {
    let mut ports = capture(ProfileKind::Exposed);
    ports.load_test = Some(LoadTestSetting(0x84));
    ports.set_halfword(0xca, 30);
    assert_eq!(frame_interval(&mut ports, PROFILE), Ok(1192 * 1585 / 54));
}

#[test]
fn the_frame_limit_never_falls_below_one_millisecond() {
    let mut ports = capture(ProfileKind::Exposed);
    ports.load_test = Some(LoadTestSetting(0x83));
    // 1585 lines * 40019 / 1000 = 63430.1, made even.
    let stretched = (40_019u32 * 15_850 / 1000 + 5) / 10 & !1;
    assert_eq!(frame_interval(&mut ports, PROFILE), Ok(1192 * stretched / 54));
}

#[test]
fn line_pair_interval_is_two_lines_in_microseconds() {
    let mut ports = capture(ProfileKind::ConfiguredAlternate);
    assert_eq!(line_pair_interval(&mut ports, PROFILE), Ok(44));
    assert_eq!(line_pair_interval(&mut capture(ProfileKind::Tabled), PROFILE), Err(Unsupported));
}

#[test]
fn a_one_second_shutter_on_an_exposed_profile_lasts_one_second() {
    let mut ports = capture(ProfileKind::Exposed);
    let interval = exposure_interval(&mut ports, PROFILE, ShutterLevel(0), RateSelection(0)).unwrap();
    assert!((999_000..=1_000_000).contains(&interval), "{interval}");
}

#[test]
fn a_short_shutter_still_takes_the_whole_frame() {
    let mut ports = capture(ProfileKind::Exposed);
    let interval = exposure_interval(&mut ports, PROFILE, ShutterLevel(1000), RateSelection(0));
    assert_eq!(interval, Ok(1192 * 1585 / 54));
}

#[test]
fn the_exposure_margin_adds_sixteenths_of_a_line() {
    let mut ports = capture(ProfileKind::Exposed);
    let plain = exposure_interval(&mut ports, PROFILE, ShutterLevel(0), RateSelection(0)).unwrap();
    ports.set_byte(0x94c2, 54);
    let margined = exposure_interval(&mut ports, PROFILE, ShutterLevel(0), RateSelection(0)).unwrap();
    // 54 sixteenths of a line are 108 lines of 1192 clocks.
    assert_eq!(margined - plain, 108 * 1192 / 54);
}

#[test]
fn a_rate_selection_divides_the_frame_by_its_rate() {
    let mut ports = capture(ProfileKind::Exposed);
    ports.channel_divisors[7] = 2999;
    // 1585 * 40019 / 2999 = 21150.4 tenths of a line, so 2115 lines.
    assert_eq!(exposure_interval(&mut ports, PROFILE, ShutterLevel(0), RateSelection(7)), Ok(1192 * 2115 / 54));
}

#[test]
fn configured_exposure_interval_is_its_frame_and_the_alternate_has_none() {
    let mut ports = capture(ProfileKind::Configured);
    ports.configured_lines = 900;
    assert_eq!(exposure_interval(&mut ports, PROFILE, ShutterLevel(0), RateSelection(0)), Ok(1192 * 900 / 54));
    let mut alternate = capture(ProfileKind::ConfiguredAlternate);
    assert_eq!(exposure_interval(&mut alternate, PROFILE, ShutterLevel(0), RateSelection(0)), Err(Unsupported));
}

#[test]
fn a_shutter_the_line_grid_nearly_realises_keeps_its_level() {
    let mut ports = capture(ProfileKind::Exposed);
    assert_eq!(quantized_shutter(&mut ports, PROFILE, ShutterLevel(600)), Ok(ShutterLevel(600)));
}

#[test]
fn a_shutter_longer_than_the_frame_is_cut_to_the_frame_less_two_lines() {
    let mut ports = capture(ProfileKind::Exposed);
    let level = quantized_shutter(&mut ports, PROFILE, ShutterLevel(0)).unwrap();
    // 1583 lines of 1192 clocks at 54 MHz last 34.94 ms: log2(1/0.03494) = 4.839 stops.
    assert_eq!(level, ShutterLevel(484));
}

#[test]
fn an_undeclared_kind_has_no_quantized_shutter() {
    let mut ports = capture(ProfileKind::Exposed);
    ports.kind = None;
    assert_eq!(quantized_shutter(&mut ports, PROFILE, ShutterLevel(600)), Err(Unsupported));
}

#[test]
fn exposed_window_takes_the_reference_left_edge_and_one_column_less() {
    let crop = Rectangle { top: 4, left: 10, width: 1600, height: 1200 };
    let mut ports = capture(ProfileKind::Exposed);
    ports.crop = crop;
    assert_eq!(readout_window(&mut ports, PROFILE, false), Rectangle { left: 60, width: 1599, ..crop });
    let mut other = capture(ProfileKind::Configured);
    other.crop = crop;
    assert_eq!(readout_window(&mut other, PROFILE, false), crop);
}

#[test]
fn left_reference_is_the_profile_reference_halfword() {
    assert_eq!(left_reference(&capture(ProfileKind::Tabled), PROFILE), 62);
}

#[test]
fn reference_scale_is_thousandths_of_the_ratio_rounded_on_the_tenth() {
    let mut ports = capture(ProfileKind::Tabled);
    ports.set_halfword(0x100, 300);
    ports.set_halfword(0x200, 1234);
    assert_eq!(reference_scale(&ports, 1, 2), 411);
    ports.set_halfword(0x200, 1236);
    assert_eq!(reference_scale(&ports, 1, 2), 412);
}

#[test]
fn reference_scale_of_a_zero_denominator_divides_by_one() {
    let mut ports = capture(ProfileKind::Tabled);
    ports.set_halfword(0x200, 7);
    assert_eq!(reference_scale(&ports, 0, 0), 700);
}

#[test]
fn converted_sample_period_runs_the_period_through_ratio_and_decade_tables() {
    let mut ports = capture(ProfileKind::Configured);
    ports.sample_period = (1000, 3);
    // (4095 - 1000) / 4095 of 10^8 against 10^8 is 24420 below; a tenth of that is step 2442 of
    // the decade table, whose fake value is ten times step 12442.
    assert_eq!(converted_sample_period(&mut ports, PROFILE), 1244);
}

#[test]
fn a_refused_ratio_counts_as_zero() {
    let mut ports = capture(ProfileKind::Configured);
    ports.sample_period = (1000, 3);
    ports.ratio_refused = true;
    assert_eq!(converted_sample_period(&mut ports, PROFILE), 1000);
}
