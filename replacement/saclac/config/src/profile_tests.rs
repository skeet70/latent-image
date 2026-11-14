use crate::fake::Area;
use crate::profile::*;

#[test]
fn a_level_index_rounds_hundredths_and_clamps() {
    for (level, index) in [(0, 0), (449, 0), (450, 1), (549, 1), (550, 2), (1000, 6), (1349, 9), (1350, 9), (5000, 9), (-700, 0)] {
        assert_eq!(level_index(level), index, "{level}");
    }
}

fn quadruple(area: Area, at: u32, values: [i16; 4]) -> Area {
    values.iter().enumerate().fold(area, |area, (index, &value)| area.with_half(at + 2 * index as u32, value as u16))
}

#[test]
fn adjusted_values_reorder_the_base_and_subtract_the_trim_without_going_negative() {
    // Level 450 is index 1: set One reads base 0x1fe9 and trim 0x20c1.
    let area = quadruple(Area::default(), 0x1fe9, [10, 20, 30, 40]);
    let area = quadruple(area, 0x20c1, [5, 25, -5, 1]);
    let set = ParameterSet::new(1);
    assert_eq!(adjusted_values(&area, set, 450, true), Adjusted { values: [20, 10, 40, 30], complete: true });
    assert_eq!(adjusted_values(&area, set, 450, false), Adjusted { values: [0, 5, 39, 35], complete: true });
}

#[test]
fn a_missing_key_reads_zeros_and_leaves_the_values_incomplete() {
    let area = quadruple(Area::default(), 0x1fe9, [10, 20, 30, 40]);
    assert_eq!(adjusted_values(&area, ParameterSet::new(1), 0, false), Adjusted { values: [0; 4], complete: false }, "index 0");
    assert_eq!(adjusted_values(&area, ParameterSet::new(3), 450, true), Adjusted { values: [0; 4], complete: false });
}

#[test]
fn each_parameter_set_uses_its_own_base_and_signed_trim() {
    let area = quadruple(Area::default(), 0x2079, [100, 200, 300, 400]);
    let area = quadruple(area, 0x2151, [3, 5, -7, 500]);
    let area = quadruple(area, 0x2031, [20, 40, 60, 80]);
    let area = quadruple(area, 0x2109, [-10, 5, 70, 9]);
    let area = quadruple(area, 0x1fe9, [1, 2, 3, 4]);
    assert_eq!(adjusted_values(&area, Some(ParameterSet::Zero), 450, false), Adjusted { values: [195, 97, 0, 307], complete: true });
    assert_eq!(adjusted_values(&area, Some(ParameterSet::Two), 450, false), Adjusted { values: [35, 30, 71, 0], complete: true });
}

#[test]
fn mode_keys_and_kind_five() {
    assert_eq!(mode_key(4), Some(0x1de7));
    assert_eq!(mode_key(5), Some(0x1de5));
    assert_eq!(mode_key(13), Some(0x1de6));
    assert_eq!(mode_key(14), None);
    assert!(is_kind_five(5));
    assert!(!is_kind_five(4));
}

#[test]
fn the_reciprocal_scale_reads_zero_as_four() {
    assert_eq!(reciprocal_scale(1), 1 << 20);
    assert_eq!(reciprocal_scale(0), 1 << 18);
    assert_eq!(reciprocal_scale(3), 349_525);
    assert_eq!(output_limit(&Area::default().with_half(0x21a3, 77)), 77);
}

fn window() -> PointWindow { PointWindow { enabled: true, x: 10, y: 20, height: 5, width: 4, x_origin: 1, y_origin: 2 } }

fn record(count: u16) -> PointRecord {
    PointRecord { source: PointSource::Adjustment(0x100), count, shift: [1, -1], mode: PointMode::Off, window: window() }
}

fn packed(x: u32, y: u32, quality: u32) -> u32 { y << 16 | quality << 14 | x }

#[test]
fn filtered_points_keep_those_in_the_window_shifted_and_less_the_origin() {
    // Shift (1, -1) and origin (1, 2) move a point by (0, -3); the window is x 10..14, y 20..25.
    let area = Area::default().with_word(0x100, packed(10, 23, 2)).with_word(0x104, packed(14, 23, 0)).with_word(0x108, packed(13, 27, 1));
    let mut buffer = [0xdead; POINT_BUFFER_WORDS];
    let mut points = record(3);
    filter_points(&area, &mut points, &mut buffer);
    assert_eq!(points.count, 2);
    assert_eq!(points.source, PointSource::Buffer);
    assert_eq!(points.shift, [0, 0]);
    assert_eq!(&buffer[..3], &[packed(10, 20, 2), packed(13, 24, 1), 0]);
}

#[test]
fn no_kept_point_empties_the_list_and_a_disabled_window_changes_nothing() {
    let area = Area::default().with_word(0x100, packed(0, 0, 0));
    let mut buffer = [7; POINT_BUFFER_WORDS];
    let mut points = record(1);
    filter_points(&area, &mut points, &mut buffer);
    assert_eq!((points.count, points.source), (0, PointSource::Adjustment(0x100)));
    assert!(buffer.iter().all(|&word| word == 0));
    let mut buffer = [7; POINT_BUFFER_WORDS];
    let mut points = PointRecord { window: PointWindow { enabled: false, ..window() }, ..record(1) };
    filter_points(&area, &mut points, &mut buffer);
    assert_eq!((points.count, buffer[0]), (1, 7));
}

#[test]
fn filtering_caps_the_published_list_at_buffer_capacity() {
    struct Points;
    impl crate::adjustment::Adjustments for Points {
        fn byte(&self, key: crate::adjustment::AdjustmentKey) -> u8 {
            let offset = key.0 - 0x100;
            (1 + offset / 4).to_le_bytes()[(offset % 4) as usize]
        }
    }
    let mut points = PointRecord {
        count: 1025,
        shift: [0, 0],
        window: PointWindow { enabled: true, width: 2048, height: 1, ..PointWindow::default() },
        ..record(0)
    };
    let mut buffer = [0; POINT_BUFFER_WORDS];
    filter_points(&Points, &mut points, &mut buffer);
    assert_eq!((points.count, points.source), (1024, PointSource::Buffer));
    assert_eq!((buffer[0], buffer[1023]), (1, 1024));
}

#[test]
fn prepared_points_read_the_index_lists_and_select_the_mode() {
    // Index 6: list key 1 at (1 + 80) << 12 with its count at 0x277d, shift pair key 1 at 0x27b4.
    let list = (1 + 80) << 12;
    let area = Area::default().with_half(0x277d, 1).with_word(list, packed(10, 23, 0));
    let area = area.with_half(0x27b4, 1).with_half(0x27b6, (-1i16) as u16).with(0xb4, 0x20);
    let mut buffer = [0; POINT_BUFFER_WORDS];
    let mut points = record(0);
    prepare_points(&area, 6, &mut points, &mut buffer);
    assert_eq!((points.count, points.mode), (1, PointMode::First));
    prepare_points(&area.with(0xc9, 0x20), 6, &mut points, &mut buffer);
    assert_eq!(points.mode, PointMode::Second);
    prepare_points(&area.with(0xb4, 0), 6, &mut points, &mut buffer);
    assert_eq!(points.mode, PointMode::Off);
    prepare_points(&area, 4, &mut points, &mut buffer);
    assert_eq!((points.count, points.mode), (0, PointMode::Off));
}
