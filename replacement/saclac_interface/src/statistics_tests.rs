use crate::statistics::{SettingOverrides, StatisticsSetting};

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
