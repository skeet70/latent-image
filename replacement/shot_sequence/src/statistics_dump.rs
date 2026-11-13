#![forbid(unsafe_code)]

//! The statistics dump a half press writes into the client frame when the adjustment asks for it.

use crate::boundary::Frame;
use crate::stock::statistics_dump as layout;

pub trait DumpBoundary {
    /// Whether the platform keeps a statistics source buffer to copy from.
    fn statistics_source_present(&mut self) -> bool;
    fn fill_dump_header(&mut self, frame: Frame);
    fn fill_frame(&mut self, frame: Frame, offset: u32, length: u32, byte: u8);
    fn write_frame_word(&mut self, frame: Frame, offset: u32, value: u32);
    fn copy_statistics(&mut self, frame: Frame, offset: u32, length: u32);
}

/// Whether a first-capture dump has tagged the frame; the capture controller's dump reads and
/// clears it too.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsDump {
    pub armed: bool,
}

impl StatisticsDump {
    /// `first_capture` is the record's dump argument 1.
    pub fn write(&mut self, frame: Frame, first_capture: bool, boundary: &mut impl DumpBoundary) {
        if !self.armed && first_capture {
            boundary.fill_dump_header(frame);
            tag(frame, boundary);
            self.armed = true;
        }
        if !boundary.statistics_source_present() {
            for (offset, length, byte) in layout::UNSOURCED_FILLS { boundary.fill_frame(frame, offset, length, byte); }
        } else if first_capture {
            boundary.copy_statistics(frame, layout::STATISTICS, layout::STATISTICS_BYTES);
        } else {
            boundary.fill_frame(frame, layout::DUMP, layout::DUMP_BYTES, 0);
            tag(frame, boundary);
            self.armed = false;
        }
    }
}

fn tag(frame: Frame, boundary: &mut impl DumpBoundary) {
    boundary.write_frame_word(frame, layout::DUMP, layout::START_TAG);
    boundary.write_frame_word(frame, layout::END_TAG_OFFSET, layout::END_TAG);
}
