#![forbid(unsafe_code)]

use xm1_saclac_interface::lens;
use xm1_saclac_interface::{Command, Completed, Message, Parameters, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildOutcome {
    Complete,
    Pending,
    /// The child had no handling for the event.
    Inactive,
    Measurement(MeasurementOutcome),
    Search(crate::search::SearchDecision),
    Fault,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Restart {
    Pending,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeasurementUpdate {
    Window,
    NoPeerRequest,
    Source,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetainedRoundStatus {
    Status03,
    Status0c,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeasurementOutcome {
    Update { update: MeasurementUpdate, restart: Restart },
    Round(RetainedRoundStatus),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeasurementControl {
    Initialize,
    Suspend,
    Resume,
    Stop,
    Reconfigure,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcquisitionControl {
    Initialize,
    Start,
    Stop,
}


/// The lens children's entries that carry no queue message; a reissued command or a
/// re-entered completion is a `ChildEvent::Command` or `ChildEvent::Completed` again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensControl {
    PrepareDrive,
    /// The root's cancellation drives the lens.
    CancelDrive,
    Setting(lens::Setting),
    /// Steps by the block of notification 0x1b.
    Step,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShotControl {
    StartS1,
    AdvanceS1,
    InitializeCapture,
    StartLiveSequence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildEvent<'a, Control> {
    Control { control: Control, parameters: Parameters<'a>, record: RecordHandle },
    Command { command: Command<'a>, record: RecordHandle },
    Completed { completed: Completed<'a>, record: RecordHandle },
}

/// The shot entry a parent routes a command or completion to; a control selects its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShotTarget { HalfPress, Capture, Live }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShotEvent<'a> {
    Control { control: ShotControl, record: RecordHandle },
    Command { target: ShotTarget, command: Command<'a>, record: RecordHandle },
    Completed { target: ShotTarget, completed: Completed<'a>, record: RecordHandle },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildRequest<'a, SearchSample> {
    Measurement(ChildEvent<'a, MeasurementControl>),
    S1Acquisition(ChildEvent<'a, AcquisitionControl>),
    S1Search(crate::search::SearchInput<SearchSample>),
    LensBringUp { message: Message<'a>, record: RecordHandle },
    LensLive(ChildEvent<'a, LensControl>),
    LensS1(ChildEvent<'a, LensControl>),
    Shot(ShotEvent<'a>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensEffect {
    MeasurementOnly,
    LensReaching,
}

impl<SearchSample> ChildRequest<'_, SearchSample> {
    pub const fn lens_effect(&self) -> LensEffect {
        match self {
            Self::S1Acquisition(_) => LensEffect::MeasurementOnly,
            Self::Measurement(_)
            | Self::S1Search(_)
            | Self::LensBringUp { .. }
            | Self::LensLive(_)
            | Self::LensS1(_)
            | Self::Shot(_) => LensEffect::LensReaching,
        }
    }
}

pub trait Children {
    type SearchSample;

    fn handle(&mut self, request: ChildRequest<'_, Self::SearchSample>) -> ChildOutcome;
}
