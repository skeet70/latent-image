#![forbid(unsafe_code)]

//! The evaluation rings the peak search, the return and the approach keep beside the
//! channel ring.

use super::sample::RingEntry;
use super::{Context, Phase};

const ENTRY: RingEntry = RingEntry { contrast: 0, area_levels: [0; 2], focus_value: 0, position: xm1_saclac_controllers::search::LensPosition(0), class: 0 };

/// Indexes into the rings are stock's signed bytes: -1 for none.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Tracks {
    pub extent: [RingEntry; 13],
    pub extent_newest: i8,
    pub extent_mark: i8,
    pub scan: [RingEntry; 7],
    pub scan_newest: i8,
    pub scan_mark: i8,
    pub marks: [u16; 7],
    pub flag: u8,
    pub index: i8,
    pub mark: u16,
    pub total: u32,
}

impl Tracks {
    /// As 0xf3c17c leaves them.
    pub(crate) const fn new() -> Self {
        Self {
            extent: [ENTRY; 13],
            extent_newest: -1,
            extent_mark: -1,
            scan: [ENTRY; 7],
            scan_newest: -1,
            scan_mark: -1,
            marks: [0; 7],
            flag: 0,
            index: 0,
            mark: 0,
            total: 0,
        }
    }
}

impl Context {
    /// 0xf41000: a direction or peak search also forgets the extent ring.
    pub(crate) fn forget_tracks(&mut self) {
        let tracks = &mut self.tracks;
        if matches!(self.phase, Phase::Direction | Phase::PeakSearch) {
            tracks.extent = [ENTRY; 13];
            tracks.extent_newest = -1;
            tracks.extent_mark = -1;
        }
        tracks.scan = [ENTRY; 7];
        tracks.scan_newest = -1;
        tracks.scan_mark = -1;
        tracks.marks = [0; 7];
        tracks.total = 0;
        tracks.flag = 0;
        tracks.index = -1;
        tracks.mark = 0;
    }
}
