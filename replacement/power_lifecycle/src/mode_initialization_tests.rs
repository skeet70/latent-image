use crate::mode_initialization::{compact_flag_index, delay_ticks, requested_mode, FlagId, Mode, ModeRequest, PowerOffConditions};

const DEFER: PowerOffConditions = PowerOffConditions { delay_nonzero: true, flag23: false, flag24: false, flag27: false };

#[test]
fn initial_and_capture_requests_preserve_higher_priority_modes() {
    for mode in [Mode::Mode7, Mode::Mode8, Mode::Mode11, Mode::Mode12, Mode::PowerOff, Mode::Mode14] {
        assert_eq!(requested_mode(mode, ModeRequest::Initial, DEFER), mode);
        assert_eq!(requested_mode(mode, ModeRequest::Alternate, DEFER), mode);
        assert_eq!(requested_mode(mode, ModeRequest::Capture, DEFER), mode);
    }
    assert_eq!(requested_mode(Mode::None, ModeRequest::Initial, DEFER), Mode::Mode3);
    assert_eq!(requested_mode(Mode::Mode3, ModeRequest::Capture, DEFER), Mode::Mode5);
    assert_eq!(requested_mode(Mode::Mode4, ModeRequest::Capture, DEFER), Mode::Mode6);
}

#[test]
fn shutdown_delay_and_each_transient_condition_choose_the_pending_mode() {
    for raw in 0..=14 {
        let mode = Mode::parse(raw).unwrap();
        assert_eq!(requested_mode(mode, ModeRequest::PowerOff, DEFER), Mode::Mode12);
        for conditions in [
            PowerOffConditions { delay_nonzero: false, ..DEFER },
            PowerOffConditions { flag23: true, ..DEFER },
            PowerOffConditions { flag24: true, ..DEFER },
            PowerOffConditions { flag27: true, ..DEFER },
        ] {
            assert_eq!(requested_mode(mode, ModeRequest::PowerOff, conditions), Mode::PowerOff);
        }
        assert_eq!(requested_mode(mode, ModeRequest::Request16, DEFER), Mode::Mode14);
    }
}

#[test]
fn delay_conversion_truncates_and_fallback_applies_only_to_zero() {
    for (input, ticks) in [(0, 0), (1, 5), (2, 11), (5, 28), (10, 56), (100, 560), (255, 1428)] {
        assert_eq!(delay_ticks(input, false), ticks);
        assert_eq!(delay_ticks(input, true), if input == 0 { 10 } else { ticks });
    }
}

#[test]
fn disjoint_flag_ranges_pack_without_gaps() {
    for (id, index) in [(1, 1), (57, 57), (257, 58), (265, 66), (512, 67), (542, 97), (768, 98), (825, 155), (1024, 156), (1066, 198), (1280, 199), (1290, 209)] {
        assert_eq!(compact_flag_index(FlagId::new(id)), index);
    }
    for id in [0, 58, 256, 266, 511, 543, 767, 826, 1023, 1067, 1279, 1291, u32::MAX] {
        assert_eq!(compact_flag_index(FlagId::new(id)), 0);
    }
}

#[test]
fn display_modes_preserve_all_permutations_and_fallbacks() {
    use crate::mode_initialization::{display_projection, DisplayMode};
    let primary = [0, 1, 2, 3, 4, 5, 9, 10, 11, 12, 13, 6, 7, 8, 14, 15, 16, 17, 18, 19, 20, 21, 22, 25, 26, 24, 23, 27, 28, 29, 30, 31, 32, 33, 34];
    let secondary = [3, 5, 5, 5, 5, 2, 1, 5, 5, 5, 5, 5, 4, 6, 5, 5, 5, 5, 5, 5, 5, 5, 0, 5, 5, 5, 0, 5, 5, 5, 0, 5, 5, 5, 5];
    for raw in 0..=34 {
        let projected = display_projection(DisplayMode::new(raw));
        assert_eq!(projected.primary, primary[raw as usize]);
        assert_eq!(projected.secondary, secondary[raw as usize]);
    }
    for raw in [35, 255, u32::MAX] {
        let projected = display_projection(DisplayMode::new(raw));
        assert_eq!(projected.primary, 1);
        assert_eq!(projected.secondary, 5);
    }
}
