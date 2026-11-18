use crate::policy::*;

#[test]
fn exposure_modes_split_into_primary_limit_and_adjustment_groups() {
    let primary: [u32; 3] = [0, 1, 4];
    let limited: [u32; 3] = [1, 2, 4];
    let adjusted: [u32; 2] = [3, 5];
    for mode in 0..16 {
        let mode = ExposureMode(mode);
        assert_eq!(mode.is_primary(), primary.contains(&mode.0), "{mode:?}");
        assert_eq!(mode.uses_primary_limit(), limited.contains(&mode.0), "{mode:?}");
        assert_eq!(mode.uses_adjustment(), adjusted.contains(&mode.0), "{mode:?}");
    }
}

#[test]
fn selection_mode_categories() {
    assert_eq!([0, 2, 5, 13, 14].map(|mode| SelectionMode(mode).category()), [1, 3, 2, 3, 1]);
}

#[test]
fn selection_kind_acceptance_and_qualification() {
    assert!(SelectionKind(52).accepted(false, false));
    assert!(!SelectionKind(2).accepted(false, false));
    assert!(SelectionKind(2).accepted(true, false));
    assert!(SelectionKind(2).accepted(false, true));

    assert!(SelectionKind(7).qualifies(false, true, false));
    assert!(!SelectionKind(6).qualifies(false, true, true));
    assert!(SelectionKind(3).qualifies(false, true, true));
    assert!(!SelectionKind(3).qualifies(false, true, false));
    assert!(!SelectionKind(7).qualifies(true, true, true));
    assert!(!SelectionKind(7).qualifies(false, false, true));
}

#[test]
fn flag_words_classify_by_their_leading_bit() {
    assert_eq!([0, 1, 2, 8, 4, 0x0f, 0x0c].map(|flags| RowFlags(flags).class()), [0, 1, 2, 2, 3, 1, 2]);
    assert!(RowFlags(0x7c).bit_two() && RowFlags(0x7c).bit_five());
    assert!(!RowFlags(0x3b).bit_two() && RowFlags(0x3b).bit_five());
    assert!(!RowFlags(0x1f).bit_five());
    assert_eq!([0, 1, 2, 3, 0x10, 0x13, 0x04].map(|flags| SelectionFlags(flags).category()), [0, 1, 2, 1, 3, 3, 0]);
}

#[test]
fn offset_position_clamps_the_step_and_wraps() {
    assert_eq!(offset_position(100, 50, true), 150);
    assert_eq!(offset_position(100, 50, false), 50);
    assert_eq!(offset_position(0, 999, true), 200);
    assert_eq!(offset_position(0, -999, true), -200);
    assert_eq!(offset_position(0, -999, false), 200);
    assert_eq!(offset_position(i16::MAX, 1, true), i16::MIN);
}

#[test]
fn raise_all_caps_at_the_limit() {
    let mut values = [1, 5, 9, -4];
    raise_all(&mut values, 3, 10);
    assert_eq!(values, [4, 8, 10, -1]);
    raise_all(&mut values, 0, 0);
    assert_eq!(values, [4, 8, 10, -1]);
}
