#![forbid(unsafe_code)]

//! The lens step a phase moves by (0xfaab3c, with its base step 0xfab390).

use super::settings::{Setting, StepKind};
use super::{Context, Platform, StepQuery};

impl Context {
    /// The step and its rate for `kind`, from the lens table.
    pub(crate) fn step_size(&mut self, platform: &mut impl Platform, kind: StepKind) -> (u16, u32) {
        let lens = platform.lens_identity();
        let low_contrast = kind == StepKind::Scan && self.values.contrast < platform.setting(Setting::ProbeContrast).wrapping_mul(10);
        let base = if low_contrast {
            platform.setting(Setting::LowContrastStep).wrapping_mul(10) as u16
        } else {
            let selected = platform.measurement_selection() == 1;
            (platform.setting(Setting::BaseStep { kind, selected }).wrapping_mul(10) & 0xfffe) as u16
        };
        let parameter_index = platform.parameter_index(self.frame.parameter);
        let fine = platform.fine_steps();
        let period = platform.profile_period(self.frame.profile_class);
        let period = period.checked_shr(platform.setting(Setting::PeriodShift)).unwrap_or(0) / 100;
        platform.lens_step(StepQuery { lens, parameter_index, base, period, lens_mode: self.bridge.lens_mode, fine })
    }
}
