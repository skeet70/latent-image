use crate::boundary::Frame;
use crate::statistics_dump::{DumpBoundary, StatisticsDump};
use std::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
enum Write {
    Header,
    Fill(u32, u32, u8),
    Word(u32, u32),
    Copy(u32, u32),
}

struct Frames {
    source: bool,
    writes: Vec<Write>,
}

impl DumpBoundary for Frames {
    fn statistics_source_present(&mut self) -> bool { self.source }
    fn fill_dump_header(&mut self, frame: Frame) {
        assert_eq!(frame, FRAME);
        self.writes.push(Write::Header);
    }
    fn fill_frame(&mut self, _frame: Frame, offset: u32, length: u32, byte: u8) { self.writes.push(Write::Fill(offset, length, byte)); }
    fn write_frame_word(&mut self, _frame: Frame, offset: u32, value: u32) { self.writes.push(Write::Word(offset, value)); }
    fn copy_statistics(&mut self, _frame: Frame, offset: u32, length: u32) { self.writes.push(Write::Copy(offset, length)); }
}

const FRAME: Frame = Frame(0x4000_0000);
const TAGS: [Write; 2] = [Write::Word(0x84f74, 0x5354_4d44), Write::Word(0x9cecc, 0x4c41_4d44)];

#[test]
fn the_first_capture_tags_the_frame_once_and_copies_the_statistics() {
    let mut dump = StatisticsDump::default();
    let mut frames = Frames { source: true, writes: Vec::new() };
    dump.write(FRAME, true, &mut frames);
    let [header, tags @ .., copy] = frames.writes.as_slice() else { panic!() };
    assert_eq!((header, tags, copy), (&Write::Header, TAGS.as_slice(), &Write::Copy(0x851fc, 0xbe68)));
    frames.writes.clear();
    dump.write(FRAME, true, &mut frames);
    assert_eq!(frames.writes, [Write::Copy(0x851fc, 0xbe68)]);
    assert!(dump.armed);
}

#[test]
fn a_later_capture_clears_the_dump_retags_it_and_disarms() {
    let mut dump = StatisticsDump { armed: true };
    let mut frames = Frames { source: true, writes: Vec::new() };
    dump.write(FRAME, false, &mut frames);
    assert_eq!(frames.writes, [Write::Fill(0x84f74, 0x17f5c, 0), TAGS[0].clone(), TAGS[1].clone()]);
    assert!(!dump.armed);
}

#[test]
fn without_a_source_the_statistics_area_is_cleared_and_marked() {
    let mut dump = StatisticsDump { armed: true };
    let mut frames = Frames { source: false, writes: Vec::new() };
    dump.write(FRAME, false, &mut frames);
    assert_eq!(
        frames.writes,
        [Write::Fill(0x851fc, 0xbe68, 0), Write::Fill(0x851fc, 0x2c00, 0xff), Write::Fill(0x87e14, 0x8400, 0xff), Write::Fill(0x90214, 0x168, 0xff)]
    );
    assert!(dump.armed);
}

