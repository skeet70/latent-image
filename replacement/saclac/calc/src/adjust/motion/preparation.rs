//! The run's preparation decisions besides its readings: what becomes of the previous focus
//! confirmation (0xf9c744), the listed areas in a capture mode's frame (0xf991b8) and whether the
//! run searches in full (0xfa4594..0xfa4638).

use xm1_saclac_profile_records::stock::record::group_01;

use crate::adjust::motion::readings::{AreaList, ListedArea, RecordReadings};
use crate::exposure::ports::{ProfileAccess, Properties};
use crate::exposure::settings;
use crate::stock::motion::{self as stock};

/// What the run makes of the previous focus confirmation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PreviousFocus {
    #[default]
    Ignored,
    /// The record's class admits it under the configured mask.
    Admitted,
    /// The confirmation stands and the run reports it without searching.
    Reported,
    /// The confirmation stands and the run checks it.
    Checked,
}

impl PreviousFocus {
    /// 0xf9c744, with `treated` when the run searches with the scene treatment. Stock also reads
    /// word 0x2c of its argument record, which 0xfa364c never writes (0xf9c750), and decides
    /// otherwise when it holds 1; the owned decision is the one for any other word.
    pub fn decide(ports: &mut (impl Properties + ProfileAccess), readings: &RecordReadings, treated: bool) -> Self {
        let mode = ports.property(stock::PREVIOUS_FOCUS_MODE);
        let [confirmed, kept, _, lens, calibration, _, held, _] = readings.confirmation;
        if confirmed != 1 || readings.lens != lens {
            return Self::Ignored;
        }
        if (held == 0 && treated) || (held == 1 && !treated) {
            return if mode == 2 { Self::Checked } else { Self::Ignored };
        }
        if mode == 2 {
            return if kept != 0 { Self::Checked } else { Self::Reported };
        }
        if kept == 0 && (mode == 1 || (mode == 3 && readings.limit_word == 1 && calibration == readings.level_step)) {
            return Self::Reported;
        }
        let at_0092: i16 = ports.profile().read(readings.record, &group_01::AT_0092);
        let level = at_0092.wrapping_add(settings::level_step(ports, 2));
        if level < settings::tenths(ports, stock::PREVIOUS_FOCUS_LEVEL) as i16 {
            return Self::Ignored;
        }
        let mask = ports.property(if readings.channel == 0 { stock::PREVIOUS_FOCUS_CLASSES } else { stock::PREVIOUS_FOCUS_CHANNEL_CLASSES });
        let class = if readings.auxiliary == 1 && readings.area_words[0] == 1 {
            0x10
        } else {
            match readings.class {
                5 => 4,
                6 => 2,
                4 => 1,
                _ => 0,
            }
        };
        if class & mask != 0 { Self::Admitted } else { Self::Ignored }
    }
}

/// A capture mode's frame (0xef5d44's third and fourth words).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameSize {
    pub height: u32,
    pub width: u32,
}

pub trait CaptureFrames {
    fn frame_size(&mut self, mode: u32) -> FrameSize;
}

impl AreaList {
    /// The areas laid out for the selection's capture mode, in `mode`'s frame (0xf991b8).
    pub fn in_mode(&self, frames: &mut impl CaptureFrames, mode: u32) -> Self {
        let from = frames.frame_size(stock::SELECTION_MODE);
        let to = frames.frame_size(mode);
        let scale = |value: u32, to: u32, from: u32| value.wrapping_mul(to).checked_div(from).unwrap_or(0);
        let mut converted = *self;
        let count = self.listed().len();
        for area in &mut converted.areas[..count] {
            *area = ListedArea {
                x: scale(area.x, to.width, from.width),
                y: scale(area.y, to.height, from.height),
                width: scale(area.width, to.width, from.width),
                height: scale(area.height, to.height, from.height),
                centre_x: scale(area.centre_x, to.width, from.width),
                centre_y: scale(area.centre_y, to.height, from.height),
                ..*area
            };
        }
        converted
    }
}

/// The lens control's state (0x76da38).
pub trait LensStatus {
    fn control_state(&mut self) -> u32;
}

/// Whether the run searches in full rather than starting from a result it already has
/// (0xfa4594..0xfa4638).
pub fn searches_in_full(ports: &mut (impl Properties + LensStatus), readings: &RecordReadings, previous: PreviousFocus) -> bool {
    !(ports.property(stock::NO_FULL_SEARCH) == 1
        || ports.control_state() == 2
        || readings.scene == stock::SCENE_WITHOUT_SEARCH
        || readings.designation[0] == 1
        || matches!(previous, PreviousFocus::Reported | PreviousFocus::Checked))
}
