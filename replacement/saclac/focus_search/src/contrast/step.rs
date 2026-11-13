#![forbid(unsafe_code)]

use super::settings::StepKind;
use super::{Context, Platform, Step, StepQuery};

impl Context {
    /// The step for `kind` at the channel contrast, with the lens mode the bridge is told.
    pub(crate) fn step_size(&self, platform: &mut impl Platform, kind: StepKind) -> Step {
        platform.step(StepQuery {
            kind,
            parameter: self.frame.parameter,
            contrast: self.values.contrast,
            profile_class: self.frame.profile_class,
            lens_mode: self.bridge.lens_mode,
        })
    }
}
