#![forbid(unsafe_code)]

//! The evaluation rings a return keeps beside the channel ring: the extent ring, which takes
//! the channel ring over once enough of its levels are high, and the scan ring, which keeps the
//! samples since the highest contrast (0xf44e38, 0xf43e90 and their helpers).

use super::sample::RingEntry;
use super::settings::Setting;
use super::{Context, FitPoint, Phase, Platform};

const ENTRY: RingEntry = RingEntry { contrast: 0, area_levels: [0; 2], focus_value: 0, position: xm1_saclac_controllers::search::LensPosition(0), class: 0 };

const EXTENT: usize = 13;
const SCAN: usize = 7;
/// The newest channel samples a rebuilt scan ring starts from.
const SCAN_SEED: usize = 4;

/// Consecutive level runs (0xf44afc): `on` raises the record's scan flag, `off` clears it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Runs {
    pub on: u16,
    pub off: u16,
}

/// Which way the return last decided; the log reports the code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReturnPath {
    None,
    Contrast,
    FocusValues,
}

impl ReturnPath {
    pub(crate) const fn code(self) -> u8 {
        match self {
            Self::None => 0xff,
            Self::Contrast => 0,
            Self::FocusValues => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Tracks {
    pub extent: [RingEntry; EXTENT],
    pub extent_newest: Option<usize>,
    pub extent_mark: Option<usize>,
    pub scan: [RingEntry; SCAN],
    pub scan_newest: Option<usize>,
    pub scan_mark: Option<usize>,
    /// Times the lens passed the extent mark since the last turn.
    pub crossings: u16,
    pub turns: u16,
    pub steep_rises: u16,
    /// The runs that need both area levels high (and either to stop) and the runs that need
    /// either (and both to stop); `TrackMarks` chooses which drives the scan flag.
    pub runs: [Runs; 2],
    /// The drive to focus aims at the extent ring rather than the scan ring.
    pub flag: u8,
    pub path: ReturnPath,
    /// The fitted extent peak.
    pub mark: u16,
    /// The focus values turned; the next decision counts it.
    pub turn: bool,
}

impl Tracks {
    /// As 0xf3c17c leaves them.
    pub(crate) const fn new() -> Self {
        Self {
            extent: [ENTRY; EXTENT],
            extent_newest: None,
            extent_mark: None,
            scan: [ENTRY; SCAN],
            scan_newest: None,
            scan_mark: None,
            crossings: 0,
            turns: 0,
            steep_rises: 0,
            runs: [Runs { on: 0, off: 0 }; 2],
            flag: 0,
            path: ReturnPath::Contrast,
            mark: 0,
            turn: false,
        }
    }

    fn forget_extent(&mut self) {
        self.extent = [ENTRY; EXTENT];
        self.extent_newest = None;
        self.extent_mark = None;
    }

    fn forget_scan(&mut self) {
        self.scan = [ENTRY; SCAN];
        self.scan_newest = None;
        self.scan_mark = None;
    }

    pub(crate) fn extent_mark_entry(&self) -> Option<RingEntry> { self.extent_mark.map(|mark| self.extent[mark]) }

    pub(crate) fn scan_mark_entry(&self) -> Option<RingEntry> { self.scan_mark.map(|mark| self.scan[mark]) }
}

impl Context {
    /// 0xf41000: a direction or peak search also forgets the extent ring.
    pub(crate) fn forget_tracks(&mut self) {
        let tracks = &mut self.tracks;
        if matches!(self.phase, Phase::Direction | Phase::PeakSearch) {
            tracks.forget_extent();
        }
        tracks.forget_scan();
        tracks.crossings = 0;
        tracks.turns = 0;
        tracks.steep_rises = 0;
        tracks.runs = [Runs::default(); 2];
        tracks.turn = false;
        tracks.flag = 0;
        tracks.path = ReturnPath::None;
        tracks.mark = 0;
    }

    /// 0xf44e38: once enough channel samples had a high level and the newest focus value fell
    /// below the extent mark's (or there is no extent ring), the channel ring becomes the
    /// extent ring; otherwise the newest sample is appended. True when it restarted.
    pub(crate) fn track_extent(&mut self, platform: &mut impl Platform) -> bool {
        let newest = self.ring.newest_entry();
        let below_mark = self.tracks.extent_mark_entry().map_or(true, |mark| (newest.focus_value as i16) < (mark.focus_value as i16));
        if below_mark && self.enough_high_levels(platform) {
            let tracks = &mut self.tracks;
            tracks.forget_extent();
            if let Some(index) = self.ring.newest {
                tracks.extent[..=index].copy_from_slice(&self.ring.entries[..=index]);
            }
            tracks.extent_newest = self.ring.newest;
            tracks.extent_mark = self.ring.newest;
            return true;
        }
        let tracks = &mut self.tracks;
        if let Some(index) = tracks.extent_newest.filter(|index| index + 1 < EXTENT) {
            tracks.extent_newest = Some(index + 1);
            tracks.extent[index + 1] = newest;
        }
        false
    }

    /// 0xf44d60: at least `RecentPeakSamples` (and one) channel samples had an area level at
    /// `RecentPeakLevel`.
    fn enough_high_levels(&self, platform: &mut impl Platform) -> bool {
        let needed = platform.setting(Setting::RecentPeakSamples);
        let Some(newest) = self.ring.newest else {
            return false;
        };
        if (newest as u32) + 1 < needed {
            return false;
        }
        let level = platform.setting(Setting::RecentPeakLevel);
        let high = self.ring.entries[..=newest].iter().filter(|entry| entry.area_levels.iter().any(|value| *value >= level)).count() as u32;
        high >= needed.max(1)
    }

    /// 0xf43e90: a sample above the scan mark's contrast rebuilds the scan ring from the
    /// newest channel samples, any other is appended. An empty ring starts from the best
    /// position a contrast decision kept, or from the channel ring.
    pub(crate) fn track_scan(&mut self) {
        let newest = self.ring.newest_entry();
        let tracks = &mut self.tracks;
        match tracks.scan_mark_entry() {
            Some(mark) if mark.contrast >= newest.contrast => {
                if let Some(index) = tracks.scan_newest.filter(|index| index + 1 < SCAN) {
                    tracks.scan_newest = Some(index + 1);
                    tracks.scan[index + 1] = newest;
                }
            }
            None if tracks.path == ReturnPath::Contrast => {
                tracks.forget_scan();
                tracks.scan[0] = RingEntry { contrast: self.search.peak_contrast, position: self.focus_position, class: self.focus_speed, ..ENTRY };
                tracks.scan_newest = Some(0);
                tracks.scan_mark = Some(0);
            }
            _ => {
                tracks.forget_scan();
                let Some(index) = self.ring.newest else {
                    return;
                };
                let count = (index + 1).min(SCAN_SEED);
                tracks.scan[..count].copy_from_slice(&self.ring.entries[index + 1 - count..=index]);
                tracks.scan_newest = Some(count - 1);
                tracks.scan_mark = Some(count - 1);
            }
        }
    }

    /// 0xf4403c: takes the extent mark and its neighbours' focus values as the fit points;
    /// false without both neighbours or with three equal values.
    pub(crate) fn fit_extent_peak(&mut self) -> bool {
        let tracks = &self.tracks;
        let (Some(newest), Some(mark)) = (tracks.extent_newest, tracks.extent_mark) else {
            return false;
        };
        if newest <= 1 || mark == 0 || mark == newest {
            return false;
        }
        let [before, best, after] = [tracks.extent[mark - 1], tracks.extent[mark], tracks.extent[mark + 1]];
        if before.focus_value == best.focus_value && after.focus_value == best.focus_value {
            return false;
        }
        let point = |entry: RingEntry| FitPoint { contrast: entry.focus_value as i16 as u32, position: entry.position.0 };
        self.search.fit_points[..3].copy_from_slice(&[point(before), point(best), point(after)]);
        true
    }

    /// 0xf453d4: the lens has run past the extent mark's position in its direction.
    pub(crate) fn passed_extent_mark(&self) -> bool {
        let Some(mark) = self.tracks.extent_mark_entry() else {
            return false;
        };
        match self.direction {
            -1 => mark.position > self.search.position,
            1 => mark.position < self.search.position,
            _ => false,
        }
    }
}
