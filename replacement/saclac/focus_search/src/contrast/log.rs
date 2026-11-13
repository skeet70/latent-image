#![forbid(unsafe_code)]

//! The focus log entry each evaluation appends (0xf47598) and the debug display that shows
//! the newest one (0xf47e00).

use super::peak_search::{percent_of, Channels};
use super::sample::RingEntry;
use super::settings::Setting;
use super::{Context, Phase, Platform};

/// One evaluation as the focus log keeps it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FocusLogEntry {
    /// The phase code, or 0xff once a focus was reported.
    pub phase: u16,
    pub profile_focus: i16,
    pub parameter: u16,
    pub snapshot_level: u16,
    pub level: i32,
    pub lens_position: i16,
    pub focus_mark: u16,
    pub pinned_setting: u16,
    pub selected: u16,
    pub combined: u32,
    pub second: u32,
    pub flag: u32,
    pub rising: u16,
    pub falling: u16,
    pub fit_position: i16,
    pub fit: u16,
    /// The lens identity plus one.
    pub lens: u32,
    pub monitor_mark: i32,
    pub movement: [u32; 2],
    /// Words the phase reports; `None` leaves the slot's earlier words.
    pub phase_words: Option<[u32; 5]>,
    pub ticks: u32,
    pub ring_contrast: u32,
    pub ring_focus_value: u16,
    pub track_flag: u16,
    pub ring_levels: [u16; 2],
    pub track_marks: [u16; 2],
    pub scan_flag: u16,
    pub track_index: u16,
    pub display_word: u16,
    pub track_mark: u16,
}

/// A ring entry as the log keeps it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LoggedSample {
    pub focus_value: u16,
    pub contrast: u32,
    pub position: i16,
}

/// The entry and the snapshot of the scan and extent rings written beside the log.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusLog {
    pub entry: FocusLogEntry,
    pub scan: [LoggedSample; 7],
    pub extent: [LoggedSample; 13],
}

/// Rows of the debug display: an 8-byte label, then the newest value and two earlier ones
/// in 8-byte columns, terminated by the last byte.
const ROWS: usize = 10;
type Row = [u8; 32];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DebugDisplay {
    initialized: bool,
    /// Evaluations since the display last refreshed.
    skipped: u16,
    rows: [Row; ROWS],
    shown: usize,
}

impl DebugDisplay {
    pub(crate) const fn new() -> Self { Self { initialized: false, skipped: 0, rows: [[0; 32]; ROWS], shown: 0 } }
}

/// How a row shows its value: a word of all ones as `-`.
enum Shown {
    Number(u32),
    Word(u32),
}

impl Context {
    /// 0xf47598, then 0xf47e00 when the debug display is on and the record's mode is 4 to 6.
    pub(crate) fn log_evaluation(&mut self, platform: &mut impl Platform) {
        if platform.setting(Setting::FocusLog) & 2 != 0 {
            let log = self.focus_log(platform);
            platform.append_focus_log(&log);
        }
        if platform.setting(Setting::DebugDisplay) == 1 && self.frame.record_mode.wrapping_sub(4) <= 2 {
            self.refresh_debug_display(platform);
        }
    }

    fn focus_log(&self, platform: &mut impl Platform) -> FocusLog {
        let newest = self.ring.newest.map(|index| self.ring.entries[index]);
        let marks = if platform.setting(Setting::TrackMarks) == 0 { [self.tracks.marks[3], self.tracks.marks[4]] } else { [self.tracks.marks[5], self.tracks.marks[6]] };
        let fit = &self.search.fit_points;
        let entry = FocusLogEntry {
            phase: if self.transition.focus_reported == 1 { 0xff } else { self.phase.code() as u16 },
            profile_focus: platform.profile_focus(),
            parameter: platform.parameter_label(self.frame.parameter),
            snapshot_level: self.frame.levels[0],
            level: i32::from(self.frame.levels[1] as i16),
            lens_position: platform.lens_position().0,
            focus_mark: self.search.focus_marks[0],
            pinned_setting: self.frame.pinned_setting,
            selected: self.frame.selected as u16,
            combined: self.values.combined[1],
            second: self.values.seconds[1],
            flag: self.values.flag,
            rising: u16::from(self.search.rising),
            falling: u16::from(self.search.falling),
            fit_position: fit[2].position,
            fit: self.search.fit,
            lens: self.lens.0.wrapping_add(1) as u32,
            monitor_mark: i32::from(self.monitor.mark as i16),
            movement: if matches!(self.phase, Phase::Direction | Phase::Focus) { self.motion.movement } else { [0; 2] },
            phase_words: self.phase_words(platform),
            ticks: platform.ticks(),
            ring_contrast: newest.map_or(0, |entry| entry.contrast),
            ring_focus_value: newest.map_or(0, |entry| entry.focus_value),
            track_flag: u16::from(self.tracks.flag),
            ring_levels: newest.map_or([0; 2], |entry| entry.area_levels.map(|level| level as u16)),
            track_marks: marks,
            scan_flag: u16::from(self.frame.scan_flag),
            track_index: u16::from(self.tracks.index as u8),
            display_word: platform.setting(Setting::FocusDisplay) as u16,
            track_mark: self.tracks.mark,
        };
        FocusLog { entry, scan: self.tracks.scan.map(logged), extent: self.tracks.extent.map(logged) }
    }

    fn phase_words(&self, platform: &mut impl Platform) -> Option<[u32; 5]> {
        const NONE: u32 = u32::MAX;
        match self.phase {
            Phase::Direction => {
                let search = &self.search;
                Some([u32::from(search.lens_seen), u32::from(search.probes), u32::from(search.waits), u32::from(search.probed), u32::from(search.decision)])
            }
            Phase::PeakSearch => Some(self.peak_words(platform, peak_search_code(self.peak.rose, self.peak.passed))),
            Phase::Monitor if self.monitor.watched == 14 => Some(self.peak_words(platform, monitor_code(self.peak.rose, self.peak.passed))),
            Phase::Peak | Phase::FocusDrive => Some([NONE; 5]),
            Phase::Focus => Some([self.focus_class(platform), u32::from(self.focus.count), u32::from(self.focus.flags), NONE, NONE]),
            Phase::Approach => Some([self.search.previous_contrast, u32::from(self.search.peak_flags[0]), u32::from(self.search.peak_flags[1]), NONE, NONE]),
            Phase::Return | Phase::Monitor => None,
        }
    }

    fn peak_words(&self, platform: &mut impl Platform, code: u32) -> [u32; 5] {
        let [first, second] = platform.area_levels();
        [u32::from(first), u32::from(second), u32::from(self.frame.scan_flag), code, u32::MAX]
    }

    /// Why a focus phase would leave focus: 1 the peak fell, 2 it rose, then outside a hold
    /// 3 a low focus contrast, 4 a low second pair, 5 and 6 the settled near word moved up or
    /// down.
    fn focus_class(&self, platform: &mut impl Platform) -> u32 {
        let focus = self.focus.contrast;
        let peak = self.search.peak_contrast;
        let margin = |platform: &mut _| {
            percent_of(peak, Platform::setting(platform, Setting::FocusChangePercent)).wrapping_add(Platform::setting(platform, Setting::FocusChangeTens).wrapping_mul(10))
        };
        if peak > margin(platform).wrapping_add(focus) {
            return 1;
        }
        if focus > margin(platform).wrapping_add(peak) {
            return 2;
        }
        let free = self.transition.hold_mode == 0;
        if focus < platform.setting(Setting::FocusLowTens).wrapping_mul(10) && free {
            return 3;
        }
        if self.values.combined[1] < platform.setting(Setting::FocusLevelTens).wrapping_mul(10) && free {
            return 4;
        }
        let (watch, reference) = (i32::from(self.frame.watch), i32::from(self.watch_reference));
        if watch > reference + platform.setting(Setting::FocusLevelTens).wrapping_mul(10) as i32 && free {
            return 5;
        }
        if reference > watch + platform.setting(Setting::FocusLevelTens).wrapping_mul(10) as i32 && free {
            return 6;
        }
        0
    }

    /// 0xf47e00: every `DebugRate` evaluations, scroll each row's values and write the newest
    /// log entry's selected fields, then show the rows.
    fn refresh_debug_display(&mut self, platform: &mut impl Platform) {
        if u32::from(self.display.skipped) < platform.setting(Setting::DebugRate) {
            self.display.skipped = self.display.skipped.wrapping_add(1);
            return;
        }
        self.display.skipped = 1;
        let display = &mut self.display;
        if !display.initialized {
            for row in &mut display.rows {
                row[..8].fill(0);
                row[8..32].fill(b' ');
                row[31] = 0;
            }
            display.initialized = true;
        }
        for row in &mut display.rows {
            row.copy_within(16..23, 24);
            row.copy_within(8..15, 16);
            row[8..16].fill(b' ');
        }
        let entry = platform.newest_focus_log();
        label(&mut self.display.rows[0], b"CAF");
        let name = self.phase_name();
        write_value(&mut self.display.rows[0], name);
        let mut cursor = 1;
        let selections = [
            (Setting::DebugRowsFirst, &FIRST_ROWS[..]),
            (Setting::DebugRowsSecond, &SECOND_ROWS[..]),
            (Setting::DebugRowsThird, &THIRD_ROWS[..]),
        ];
        'rows: for (setting, rows) in selections {
            for (bit, (name, field)) in rows.iter().enumerate() {
                if platform.setting(setting) & (1 << bit) == 0 {
                    continue;
                }
                let shown = field(&entry, self);
                let row = &mut self.display.rows[cursor];
                label(row, name);
                let mut digits = [0u8; 10];
                match shown {
                    Shown::Word(u32::MAX) => write_value(row, b"-"),
                    Shown::Word(value) | Shown::Number(value) => write_value(row, decimal(value, &mut digits)),
                }
                cursor += 1;
                if cursor > 9 {
                    break 'rows;
                }
            }
        }
        self.display.shown = cursor;
        for (index, row) in self.display.rows.iter().take(self.display.shown).enumerate() {
            let line = index as u8 + 1;
            platform.show_debug_text(until_nul(&row[..8]), 1, line);
            platform.show_debug_text(until_nul(&row[8..]), 9, line);
        }
    }

    fn phase_name(&self) -> &'static [u8] {
        match self.phase {
            Phase::Direction => b"DIRTION",
            Phase::PeakSearch => b"PEK_SRH",
            Phase::Peak => b"PEAK",
            Phase::Return => b"WOB",
            Phase::Focus => b"FOCUS",
            Phase::FocusDrive => b"FCS_DRI",
            Phase::Approach => b"REMOVE",
            Phase::Monitor if self.monitor.watched == 14 => b"PEK_SRH",
            Phase::Monitor => b"WOB",
        }
    }
}

type Field = fn(&FocusLogEntry, &Context) -> Shown;

const FIRST_ROWS: [(&[u8], Field); 8] = [
    (b"ZOOM", |entry, _| Shown::Number(entry.lens)),
    (b"BV", |entry, _| Shown::Number(entry.profile_focus as i32 as u32)),
    (b"AV", |entry, _| Shown::Number(entry.parameter as i16 as i32 as u32)),
    (b"TV", |entry, _| Shown::Number(entry.snapshot_level as i16 as i32 as u32)),
    (b"SV", |entry, _| Shown::Number(entry.level as u32)),
    (b"FOCUS", |entry, _| Shown::Number(i32::from(entry.lens_position) as u32)),
    (b"VAL_LO", |entry, _| Shown::Number(entry.combined)),
    (b"VAL_HI", |entry, _| Shown::Number(entry.second)),
];

const SECOND_ROWS: [(&[u8], Field); 8] = [
    (b"REV_V_L", |entry, _| Shown::Number(entry.flag)),
    // The log's byte at +0x20 comes from C+0x213, which nothing in image 5 writes.
    (b"REV_V_H", |_, _| Shown::Number(0)),
    (b"FAC_PIN", |entry, _| Shown::Number(entry.focus_mark as i16 as i32 as u32)),
    (b"FAC_BLK", |entry, _| Shown::Number(u32::from(entry.pinned_setting))),
    (b"CAF_ARA", |entry, _| Shown::Number(u32::from(entry.selected))),
    (b"P_PK_SH", |_, context| Shown::Number(u32::from(context.frame.scan_flag))),
    (b"CMP_FCS", |entry, _| Shown::Number(entry.fit as i16 as i32 as u32)),
    (b"TICK", |entry, _| Shown::Number(entry.ticks)),
];

const THIRD_ROWS: [(&[u8], Field); 5] = [
    (b"VAL_1", |entry, _| Shown::Word(entry.phase_words.unwrap_or_default()[0])),
    (b"VAL_2", |entry, _| Shown::Word(entry.phase_words.unwrap_or_default()[1])),
    (b"VAL_3", |entry, _| Shown::Word(entry.phase_words.unwrap_or_default()[2])),
    (b"VAL_4", |entry, _| Shown::Word(entry.phase_words.unwrap_or_default()[3])),
    (b"VAL_5", |entry, _| Shown::Word(entry.phase_words.unwrap_or_default()[4])),
];

fn label(row: &mut Row, name: &[u8]) {
    row[..name.len()].copy_from_slice(name);
    row[name.len()] = 0;
}

/// Copies `text` over the newest value column without a terminator; a long value runs into
/// the earlier columns.
fn write_value(row: &mut Row, text: &[u8]) { row[8..8 + text.len()].copy_from_slice(text); }

/// The runtime's decimal text (0x722218 with flags 4): no padding, and nothing for zero.
fn decimal(mut value: u32, digits: &mut [u8; 10]) -> &[u8] {
    let mut count = 0;
    let mut divisor = 1_000_000_000;
    while divisor != 0 {
        let digit = (value / divisor) as u8;
        if digit != 0 || count != 0 {
            digits[count] = b'0' + digit;
            count += 1;
        }
        value %= divisor;
        divisor /= 10;
    }
    &digits[..count]
}

fn until_nul(bytes: &[u8]) -> &[u8] { bytes.split(|byte| *byte == 0).next().unwrap_or(bytes) }

fn logged(entry: RingEntry) -> LoggedSample { LoggedSample { focus_value: entry.focus_value, contrast: entry.contrast, position: entry.position.0 } }

/// The peak search's rise and pass marks: 1 passed only, 2 rose only, 3 both.
fn peak_search_code(rose: Channels, passed: Channels) -> u32 {
    match (rose != Channels::NONE, passed != Channels::NONE) {
        (false, false) => 0,
        (false, true) => 1,
        (true, false) => 2,
        (true, true) => 3,
    }
}

/// The monitor's reading of the same marks: 3 only when both are exactly the paired mark.
fn monitor_code(rose: Channels, passed: Channels) -> u32 {
    match (rose, passed) {
        (Channels::NONE, Channels::NONE) => 0,
        (Channels::SECONDS, Channels::SECONDS) => 3,
        (_, Channels::SECONDS) => 1,
        _ => 2,
    }
}

impl Phase {
    /// The code stock's records and traces carry.
    pub(crate) const fn code(self) -> u32 {
        match self {
            Self::Direction => 13,
            Self::PeakSearch => 14,
            Self::Peak => 15,
            Self::Return => 16,
            Self::Focus => 17,
            Self::FocusDrive => 18,
            Self::Approach => 19,
            Self::Monitor => 20,
        }
    }
}
