use crate::ports::{AbortStatus, HalfPressMode, HalfPressNotifications};
use crate::stock;
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
