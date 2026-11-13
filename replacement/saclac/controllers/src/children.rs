#![forbid(unsafe_code)]

use xm1_saclac_interface::{Command, Completed, Message, Parameters};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildOutcome {
    Complete,
    Pending,
    Fault,
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
pub enum SearchControl {
    Initialize,
    Start,
    Stop,
    Prepare,
    Evaluate,
    Cancel,
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
    Control { control: Control, parameters: Parameters<'a> },
    Command(Command<'a>),
    Completed(Completed<'a>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildRequest<'a> {
    Measurement(ChildEvent<'a, MeasurementControl>),
    S1Acquisition(ChildEvent<'a, AcquisitionControl>),
    S1Search(ChildEvent<'a, SearchControl>),
    LensBringUp(Message<'a>),
    LensLive(ChildEvent<'a, LensControl>),
    LensS1(ChildEvent<'a, LensControl>),
    Shot(ChildEvent<'a, ShotControl>),
}

pub trait Children {
    fn handle(&mut self, request: ChildRequest<'_>) -> ChildOutcome;
}
