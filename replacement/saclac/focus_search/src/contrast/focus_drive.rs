#![forbid(unsafe_code)]

//! The focus drive: drive to the focus position, then focus (0xf3d904 with 0xf3d728,
//! 0xf3d818 and 0xf3d868 for a pinned record).

use super::settings::Setting;
use super::{Context, LensPosition, Phase, Platform};

impl Context {
    /// 0xf3d904.
    pub(crate) fn focus_drive_step(&mut self, platform: &mut impl Platform) -> Phase {
        self.bridge.speed = 2;
        if self.frame.pinned != 1 {
            return self.drive_then_focus(platform);
        }
        let paced = self.pacing[2] == self.pacing[1];
        if self.holding() {
            if platform.setting(Setting::HoldDrives) != 1 {
                self.hold = true;
                self.restart_samples();
                return Phase::Focus;
            }
            if !paced {
                self.restart_samples();
                self.transition.hold_mode = 2;
                self.focus_position = LensPosition(self.limits.offset as i16);
                return Phase::FocusDrive;
            }
        } else if !paced {
            self.restart_samples();
            self.search.probed = 0;
            return Phase::Direction;
        }
        self.drive_then_focus(platform)
    }

    /// 0xf3d728: once at the focus position, focus heading back or on as `FocusDriveTurn`
    /// bit 0 and `FocusDriveKeeps` choose.
    fn drive_then_focus(&mut self, platform: &mut impl Platform) -> Phase {
        self.peak.directed = false;
        if !self.drive_to_focus() {
            return Phase::FocusDrive;
        }
        self.samples = 0;
        self.hold = true;
        let turn_bit = platform.setting(Setting::FocusDriveTurn) & 1 != 0;
        let turns = if platform.setting(Setting::FocusDriveKeeps) == 0 { !turn_bit } else { turn_bit };
        self.direction = if turns { self.previous_direction.wrapping_neg() } else { self.previous_direction };
        Phase::Focus
    }

    fn restart_samples(&mut self) {
        self.step = 0;
        self.samples = 0;
        self.direction = self.previous_direction;
    }
}
