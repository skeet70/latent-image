use crate::image_registers::*;

#[test]
fn window_limits_fill_three_fifteen_bit_fields() {
    let words = pack_window_limits(WindowLimits([0x1234, 0x0567, 0x7fff]), [0, 0]);
    assert_eq!(words, [0x0567_1234, 0x7fff]);
}

#[test]
fn a_negative_window_limit_packs_as_the_largest_field() {
    let words = pack_window_limits(WindowLimits([-1, i16::MIN, -2]), [0, 0]);
    assert_eq!(words, [0x7fff_7fff, 0x7fff]);
}

#[test]
fn window_limits_keep_the_bits_outside_their_fields() {
    let words = pack_window_limits(WindowLimits([0, 0, 0]), [u32::MAX, u32::MAX]);
    assert_eq!(words, [0x8000_8000, 0xffff_8000]);
    let words = pack_window_limits(WindowLimits([1, 2, 3]), [0x8000_8000, 0xabcd_0000]);
    assert_eq!(words, [0x8002_8001, 0xabcd_0003]);
}

#[derive(Default)]
struct Snapshot {
    words: [u32; 2],
    retained: Vec<(RegisterGroup, u8)>,
}

impl RegisterSnapshot for Snapshot {
    type Configuration = u8;

    fn restore(&mut self, _: RegisterGroup) {}

    fn window_limits(&self, configuration: u8) -> WindowLimits { WindowLimits([i16::from(configuration), -1, 9]) }

    fn window_words(&self) -> [u32; 2] { self.words }

    fn set_window_words(&mut self, words: [u32; 2]) { self.words = words }

    fn pack_retained(&mut self, group: RegisterGroup, configuration: u8) { self.retained.push((group, configuration)) }
}

#[test]
fn group_two_packs_its_window_limits_into_the_snapshot() {
    let mut snapshot = Snapshot { words: [0x8000_0000, 0xffff_ffff], ..Snapshot::default() };
    pack(RegisterGroup::Group2, 5, &mut snapshot);
    assert_eq!(snapshot.words, [0xffff_0005, 0xffff_8009]);
    assert!(snapshot.retained.is_empty());
}

#[test]
fn every_other_group_packs_with_its_retained_packer() {
    let others: Vec<_> = RegisterGroup::ALL.into_iter().filter(|group| *group != RegisterGroup::Group2).collect();
    let mut snapshot = Snapshot::default();
    for group in &others {
        pack(*group, 7, &mut snapshot);
    }
    assert_eq!(snapshot.retained, others.iter().map(|group| (*group, 7)).collect::<Vec<_>>());
    assert_eq!(snapshot.words, [0, 0]);
}

#[test]
fn groups_are_numbered_in_pipeline_order() {
    let indices: Vec<_> = RegisterGroup::ALL.into_iter().map(RegisterGroup::index).collect();
    assert_eq!(indices, (0..15).collect::<Vec<_>>());
}
