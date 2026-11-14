use crate::fake::Area;
use crate::table::*;

const BANK_SIZE: u32 = 0x3e0f82;
const WORDS: u32 = 0x3f2400;
const CODES: u32 = 0x3f4600;
const VALUES: u32 = 0x3e2530;

fn with_word(area: Area, row: u32, column: u32, word: i16) -> Area { area.with_half(WORDS + 2 * (31 * row + column), word as u16) }

fn with_value(area: Area, row: u32, column: u32, code: u8, value: u16) -> Area {
    area.with(CODES + 31 * row + column, code).with_half(VALUES + 2 * u32::from(code), value)
}

#[test]
fn a_bank_holds_the_configured_steps_and_one_when_the_size_is_out_of_range() {
    for (byte, size) in [(0, 1), (1, 1), (17, 17), (31, 31), (32, 1), (255, 1)] {
        assert_eq!(bank_size(&Area::default().with(BANK_SIZE, byte)), size, "{byte}");
    }
}

#[test]
fn steps_number_the_first_bank_from_zero_and_the_second_from_thirty_one() {
    let area = Area::default().with(BANK_SIZE, 5);
    for (step, bank, column) in [
        (0, Bank::First, 0),
        (4, Bank::First, 4),
        (5, Bank::Outside, 0),
        (30, Bank::Outside, 0),
        (31, Bank::Second, 0),
        (35, Bank::Second, 4),
        (36, Bank::Outside, 0),
    ] {
        assert_eq!((super::table::bank(&area, Step(step)), super::table::column(&area, Step(step))), (bank, column), "{step}");
    }
}

#[test]
fn words_are_read_by_row_and_column_and_rounded_half_away_from_zero() {
    let area = Area::default().with(BANK_SIZE, 3);
    let area = with_word(area, 2, 1, 15);
    let area = with_word(area, 2, 2, -15);
    let area = with_word(area, 0, 1, 14);
    let area = with_word(area, 1, 2, -14);
    assert_eq!(word(&area, Step(32), Row(2)), 15);
    assert_eq!(rounded_word(&area, Step(32), Row(2)), 2);
    assert_eq!(rounded_word(&area, Step(2), Row(2)), -2);
    assert_eq!(primary_rounded_word(&area, Step(1)), 1);
    assert_eq!(rounded_word(&area, Step(2), Row(1)), -1);
}

#[test]
fn a_relative_word_is_the_rows_first_word_less_row_zeros() {
    let area = with_word(with_word(Area::default(), 0, 0, 100), 3, 0, 255);
    assert_eq!(relative_word(&area, Row(3)), 16);
    assert_eq!(relative_word(&area, Row(0)), 0);
}

#[test]
fn value_code_255_is_one_hundred_and_other_codes_select_a_halfword() {
    let area = with_value(Area::default().with(BANK_SIZE, 4), 1, 3, 7, 0x1234);
    assert_eq!(value(&area, Step(3), Row(1)), 0x1234);
    let area = area.with(CODES + 31 + 2, 255);
    assert_eq!(value(&area, Step(2), Row(1)), 100);
}

fn corrections(entries: &[(u32, i8)]) -> Area {
    entries.iter().fold(Area::default(), |area, &(index, value)| area.with(0x1d71 + index, value as u8))
}

#[test]
fn a_correction_interpolates_between_its_bracketing_thresholds() {
    // Thresholds -100, -60, -30, 0, 30 (indices 0..=4) and 1060 last (index 35).
    let area = corrections(&[(0, -8), (1, -4), (3, 6), (4, 12), (35, 50)]);
    assert_eq!(correction(&area, -101), -8);
    assert_eq!(correction(&area, -100), -8);
    assert_eq!(correction(&area, -80), -6);
    assert_eq!(correction(&area, 15), 9);
    assert_eq!(correction(&area, 0), 6);
    assert_eq!(correction(&area, 1060), 50);
    assert_eq!(correction(&area, 5000), 50);
}

#[test]
fn a_compensated_word_adds_the_bank_offset_and_keeps_within_the_limits() {
    let base = Area::default().with(BANK_SIZE, 2).with(0x1db3, 5).with(0x1db4, 1).with(0x1db5, 4).with(0x1db6, 2);
    let first = base.with(0x3e0200 + 1, 20).with(0x3e0220 + 1, (-30i8) as u8);
    assert_eq!(compensated_word(&first, Step(1), Row(0)), 20, "within 10..=50");
    let high = first.with(0x3e0200 + 1, 90);
    assert_eq!(compensated_word(&high, Step(1), Row(0)), 50, "capped at 5 * 10");
    let low = high.with(0x3e0200 + 1, 3);
    assert_eq!(compensated_word(&low, Step(1), Row(0)), 10, "raised to 1 * 10");
    assert_eq!(compensated_word(&first, Step(32), Row(0)), -30, "second bank, magnitude within 20..=40");
    let deep = first.with(0x3e0220 + 1, (-90i8) as u8);
    assert_eq!(compensated_word(&deep, Step(32), Row(0)), -40);
    let shallow = first.with(0x3e0220 + 1, (-5i8) as u8);
    assert_eq!(compensated_word(&shallow, Step(32), Row(0)), -20);
    assert_eq!(compensated_word(&first.with(0x3e0009, 1), Step(1), Row(0)), 0, "disabled");
}

#[test]
fn a_compensated_word_includes_the_correction_of_the_rounded_word() {
    // Rounded word 0 sits on threshold 0, whose correction is entry 3.
    let area = Area::default().with(BANK_SIZE, 1).with(0x1db3, 100).with(0x1d71 + 3, 7).with(0x3e0200, 2);
    assert_eq!(compensated_word(&area, Step(0), Row(0)), 9);
    assert_eq!(combined_word(&with_word(area, 0, 0, 120), Step(0), Row(0)), 12 + 2 + 5, "the correction falls from 7 at 0 toward 0 at 30");
}

fn searchable() -> Area {
    // Row 1 runs columns 2..=6 with values 10, 20, 30, 40, 50.
    let area = Area::default().with(BANK_SIZE, 10).with(0x3e22c8 + 1, 2).with(0x3e232d + 1, 6);
    (2..=6).fold(area, |area, column| with_value(area, 1, column, column as u8, 10 * (column as u16 - 1)))
}

#[test]
fn searches_find_the_first_step_reaching_and_the_last_within_a_limit() {
    let area = searchable();
    assert_eq!(first_reaching(&area, Row(1), 25, Bounds::Primary), Step(4));
    assert_eq!(first_reaching(&area, Row(1), 10, Bounds::Primary), Step(2));
    assert_eq!(first_reaching(&area, Row(1), 99, Bounds::Primary), Step(6));
    assert_eq!(last_within(&area, Row(1), 25, Bounds::Primary), Step(3));
    assert_eq!(last_within(&area, Row(1), 50, Bounds::Primary), Step(6));
    assert_eq!(last_within(&area, Row(1), 0, Bounds::Primary), Step(2));
    assert_eq!(lower_step(&area, Row(1), StepLimit::Unbounded, Bounds::Selected), Step(2));
    assert_eq!(upper_step(&area, Row(1), StepLimit::Unbounded, Bounds::Selected), Step(6));
    assert_eq!(lower_step(&area, Row(1), StepLimit::Value(35), Bounds::Selected), Step(5));
    assert_eq!(upper_step(&area, Row(1), StepLimit::Value(35), Bounds::Selected), Step(4));
}

#[test]
fn stock_limit_words_distinguish_the_sentinel_from_numeric_limits() {
    let area = searchable();
    let lower = |word| lower_step(&area, Row(1), crate::stock::decode_step_limit(word), Bounds::Primary);
    let upper = |word| upper_step(&area, Row(1), crate::stock::decode_step_limit(word), Bounds::Primary);
    assert_eq!((lower(0xffff), upper(0xffff)), (Step(2), Step(6)));
    assert_eq!((lower(35), upper(35)), (Step(5), Step(4)));
    assert_eq!((lower(0), upper(0)), (Step(2), Step(2)));
    assert_eq!((lower(0xfffe), upper(0xfffe)), (Step(6), Step(6)));
    assert_eq!((lower(0x1ffff), upper(0x1ffff)), (Step(6), Step(6)));
}

#[test]
fn empty_bounds_give_the_bound_the_search_starts_from() {
    let area = searchable().with(0x3e22c8 + 1, 6).with(0x3e232d + 1, 6);
    assert_eq!(first_reaching(&area, Row(1), 99, Bounds::Primary), Step(6));
    let area = area.with(0x3e22c8 + 1, 7);
    assert_eq!(first_reaching(&area, Row(1), 0, Bounds::Primary), Step(7));
    assert_eq!(last_within(&area, Row(1), 0, Bounds::Primary), Step(6));
}

#[test]
fn the_selected_bounds_follow_the_alternate_bounds_while_selected() {
    let area = searchable().with(0x3e47c6 + 1, 3).with(0x3e4761 + 1, 5);
    assert_eq!(lower_step(&area, Row(1), StepLimit::Unbounded, Bounds::Selected), Step(2));
    let area = area.with(0x3e4760, 1);
    assert_eq!(lower_step(&area, Row(1), StepLimit::Unbounded, Bounds::Selected), Step(3));
    assert_eq!(upper_step(&area, Row(1), StepLimit::Unbounded, Bounds::Selected), Step(5));
    assert_eq!(upper_step(&area, Row(1), StepLimit::Value(99), Bounds::Selected), Step(5));
    assert_eq!(first_reaching(&area, Row(1), 0, Bounds::Primary), Step(2), "primary ignores the selector");
    assert_eq!(lower_step(&area, Row(1), StepLimit::Unbounded, Bounds::Primary), Step(2), "the primary low bound ignores the selector");
    assert_eq!(upper_step(&area, Row(1), StepLimit::Unbounded, Bounds::Primary), Step(6));
}

#[test]
fn the_nearest_step_picks_the_closer_value_and_the_upper_step_on_a_tie() {
    // Row 0 is the source: column 1 clamped into 1..=3 has value 24.
    let area = searchable().with(0x3e22c8, 1).with(0x3e232d, 3);
    let area = with_value(area, 0, 1, 20, 24);
    assert_eq!(nearest_step(&area, Row(0), Step(0), Row(1)), Step(3), "24 is 4 from 20 and 6 from 30");
    let area = with_value(area, 0, 1, 20, 26);
    assert_eq!(nearest_step(&area, Row(0), Step(0), Row(1)), Step(4));
    let area = with_value(area, 0, 1, 20, 25);
    assert_eq!(nearest_step(&area, Row(0), Step(0), Row(1)), Step(3), "a tie takes the upper search's step");
    assert_eq!(nearest_step(&area.with(0x3e0009, 1), Row(0), Step(0), Row(1)), Step(0));
}
