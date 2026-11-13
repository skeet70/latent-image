#![forbid(unsafe_code)]

//! The peak phase: drive to where the peak search held, then focus or return
//! (0xf3da6c with 0xf3d968), and the drive to the focus position the hold and focus drive
//! phases share (0xf3d608).

use super::settings::Setting;
use super::{Context, Phase, Platform};

impl Context {
    /// 0xf3da6c.
    pub(crate) fn peak_step(&mut self, platform: &mut impl Platform) -> Phase {
        self.bridge.speed = 2;
        if self.holding() {
            return self.hold_lens(platform);
        }
        let peaked = !self.peak.directed || self.values.level >= platform.setting(Setting::LevelUnits);
        if peaked && !self.drive_to_focus() {
            return Phase::Peak;
        }
        let next = if platform.setting(Setting::PeakReturns) == 0 || platform.setting(Setting::TrendOverride) == 0 || self.transition.hold_mode != 0 {
            self.hold = true;
            Phase::Focus
        } else {
            self.transition.return_ready = 1;
            Phase::Return
        };
        self.samples = 0;
        self.direction = if self.peak.directed { self.peak.direction } else { self.previous_direction.wrapping_neg() };
        self.peak.directed = false;
        next
    }

    /// 0xf3d608: on the phase's first sample, or after the bridge reported anything but a
    /// move, aims the next step at the focus position (clamped into the travel limits);
    /// true once the lens is there.
    pub(crate) fn drive_to_focus(&mut self) -> bool {
        if self.samples != 1 && self.bridge.last_report == xm1_saclac_interface::search::BridgeStatus::Moved {
            self.direction = self.previous_direction;
            self.step = 0;
            return true;
        }
        if self.focus_position < self.limits.low {
            self.focus_position = self.limits.low;
            self.focus_speed = 2;
        } else if self.focus_position > self.limits.high {
            self.focus_position = self.limits.high;
            self.focus_speed = 2;
        }
        self.direction = if self.focus_position > self.positions[0] { 1 } else { -1 };
        self.step = (self.focus_position.0.wrapping_sub(self.positions[0].0)).wrapping_mul(i16::from(self.direction)) as u16;
        self.bridge.speed = self.focus_speed;
        false
    }
}
