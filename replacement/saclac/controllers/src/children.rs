#![forbid(unsafe_code)]

use xm1_saclac_interface::{Command, Completed, Message, Parameters, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildOutcome {
    Complete,
    Pending,
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


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensControl {
    Initialize,
    Synchronize,
    Finish,
    S1Release,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildRequest<'a, SearchSample> {
    Measurement(ChildEvent<'a, MeasurementControl>),
    S1Acquisition(ChildEvent<'a, AcquisitionControl>),
    S1Search(crate::search::SearchInput<SearchSample>),
    LensBringUp(Message<'a>),
    LensLive(ChildEvent<'a, LensControl>),
    LensS1(ChildEvent<'a, LensControl>),
    Shot(ChildEvent<'a, ShotControl>),
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
            | Self::LensBringUp(_)
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
