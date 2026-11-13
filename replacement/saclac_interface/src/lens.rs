#![forbid(unsafe_code)]

//! Payloads of the lensdc requests saclac's lens machines post to lens control (queue 50).

use crate::{Completion, LensDcOperation, ParameterValue, RecordHandle};

/// The word the record layer packs from a record for lens control (X-M1 1.50 0x75a7c4 with
/// field 0x3f, slot 3 and tag 0x81).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileSlotReference(pub u32);

/// A lens mode word read from a command block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensMode(pub u32);

/// A signed lens position step, the halfword of notification 0x1b's block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PositionStep(pub i16);

/// The three-word requests a lens setting or profile change sends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Setting { Request04, Request05, Request06 }

/// The child whose retained completion callback a drive names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriveOwner { Live, S1 }

/// Which selection code a 0x4211 request carries: the adjustment configuration's first
/// parameter is zero or not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionCode { Unconfigured, Configured }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensPayload {
    /// The LensBringUp command's words 1..5 around the record.
    BringUp { parameters: [ParameterValue; 5], record: RecordHandle },
    /// The LensStop command's words 1 and 2 and the record.
    Stop { parameters: [ParameterValue; 2], record: RecordHandle },
    Setting { setting: Setting, slot: ProfileSlotReference, mode: LensMode },
    /// The LensDrive command's words 1..4 as sent; the live child may replace the fourth.
    Drive { owner: DriveOwner, parameters: [ParameterValue; 4], slot: ProfileSlotReference },
    /// The Cancel command's word 1 and the record; no completion callback.
    Finish { value: ParameterValue, record: RecordHandle },
    Halt,
    Step(PositionStep),
    Select { value: ParameterValue, code: SelectionCode, record: RecordHandle },
    Release,
}

impl LensPayload {
    pub const fn operation(&self) -> LensDcOperation {
        match self {
            Self::BringUp { .. } => LensDcOperation::Request02,
            Self::Stop { .. } => LensDcOperation::Request03,
            Self::Setting { setting: Setting::Request04, .. } => LensDcOperation::Request04,
            Self::Setting { setting: Setting::Request05, .. } => LensDcOperation::Request05,
            Self::Setting { setting: Setting::Request06, .. } => LensDcOperation::Request06,
            Self::Drive { .. } => LensDcOperation::Request07,
            Self::Finish { .. } => LensDcOperation::Request08,
            Self::Halt => LensDcOperation::Request09,
            Self::Step(_) => LensDcOperation::Request0b,
            Self::Select { .. } => LensDcOperation::Request11,
            Self::Release => LensDcOperation::Request12,
        }
    }

    /// The completions the request's callback posts back to queue 43.
    pub const fn completions(&self) -> &'static [Completion] {
        match self {
            Self::BringUp { .. } => &[Completion::Lens31, Completion::Lens32, Completion::Lens2f],
            Self::Stop { .. } => &[Completion::Lens30],
            Self::Setting { setting: Setting::Request04, .. } => &[Completion::Lens38],
            Self::Setting { setting: Setting::Request05, .. } => &[Completion::Lens36],
            Self::Setting { setting: Setting::Request06, .. } => &[Completion::Lens39],
            Self::Drive { .. } => &[Completion::Lens33, Completion::Lens34],
            Self::Step(_) => &[Completion::Lens35],
            Self::Select { .. } => &[Completion::Lens3d],
            Self::Finish { .. } | Self::Halt | Self::Release => &[],
        }
    }
}
