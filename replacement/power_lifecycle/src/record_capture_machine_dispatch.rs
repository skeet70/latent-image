//! X-M1 1.50 capture event routing, recovered from the twelve state rows at 0x01002bf4.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum CaptureState { Idle, Selecting, Preparing, PreparingAlternate, Exposing, Reading, Finishing, Pending, Armed, Cancelling, Draining, Restoring }
impl TryFrom<u32> for CaptureState {
    type Error = InvalidCaptureState;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value { 0 => Ok(Self::Idle), 1 => Ok(Self::Selecting), 2 => Ok(Self::Preparing), 3 => Ok(Self::PreparingAlternate), 4 => Ok(Self::Exposing), 5 => Ok(Self::Reading), 6 => Ok(Self::Finishing), 7 => Ok(Self::Pending), 8 => Ok(Self::Armed), 9 => Ok(Self::Cancelling), 10 => Ok(Self::Draining), 11 => Ok(Self::Restoring), _ => Err(InvalidCaptureState(value)) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidCaptureState(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureEvent(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureHandler {
    Select, Prepare, ExposureProgress, ReadProgress, StartArmed, StartPending,
    PrepareTick, AlternateTick, RestoreTick, BeginCancel, Abort, Stop, Cancel,
    Notify, NotifyPreparing, NotifySelecting, Parameter,
    ParameterReading, Result, ResultAlternate, Timer, Failure, Reset,
    FinishTick, DrainTick, DrainParameter, DrainResult, RestoreParameter,
    RestoreAbort, RestoreStop, RestoreResult, RestoreTimer, ResultReading, CancelTimer,
}

pub fn route(state: CaptureState, event: CaptureEvent) -> Option<CaptureHandler> {
    use CaptureHandler::*;
    use CaptureState::*;
    let event = event.0;
    if event == 0x3f4f { return Some(Failure); }
    Some(match state {
        Idle => match event { 0x3f49 => Select, 0x3f4a => Notify, _ => return None },
        Selecting => match event { 0x3f0b => Prepare, 0x3f4a => NotifySelecting, _ => return None },
        Preparing => match event { 0x3f00 => PrepareTick, 0x3f4a => NotifyPreparing, 0x3f50 => BeginCancel, 0x3f02 => Stop, 0x3f53 => Reset, 0x3f54 => Timer, _ => return None },
        PreparingAlternate => match event { 0x3f00 => AlternateTick, 0x3f4a => NotifySelecting, _ => return None },
        Exposing => match event { 0x3f00 => ExposureProgress, 0x3f4a => NotifyPreparing, _ => return None },
        Reading => match event { 0x3f00 => ReadProgress, 0x3f07 => Result, 0x3f08 => ResultAlternate, 0x3f4a => Notify, 0x3f4e => ParameterReading, 0x3f4d => Parameter, 0x3f51 => RestoreResult, _ => return None },
        Finishing => match event { 0x3f00 => FinishTick, 0x3f07 => Result, 0x3f08 => ResultAlternate, 0x3f4a => Notify, 0x3f4e => ParameterReading, 0x3f4d => Parameter, 0x3f50 => Cancel, 0x3f03 => Abort, 0x3f02 => Stop, 0x3f53 => Reset, 0x3f54 => Timer, _ => return None },
        Pending => match event { 0x3f07 => Result, 0x3f08 => ResultAlternate, 0x3f4a => Notify, 0x3f4e => ResultReading, 0x3f4d => Parameter, 0x3f03 => Abort, _ => return None },
        Armed => match event { 0x3f49 => StartArmed, 0x3f00 => StartPending, 0x3f4a => Notify, 0x3f50 => Cancel, 0x3f03 => Abort, 0x3f02 => Stop, 0x3f53 => Reset, 0x3f54 => Timer, _ => return None },
        Cancelling => match event { 0x3f4a => Notify, 0x3f4e => ResultReading, 0x3f4d => Parameter, 0x3f50 => Cancel, 0x3f02 => Stop, 0x3f52 => CancelTimer, 0x3f53 => Reset, 0x3f54 => Timer, _ => return None },
        Draining => match event { 0x3f00 => DrainTick, 0x3f4a => Notify, 0x3f07 => DrainParameter, 0x3f08 => DrainResult, _ => return None },
        Restoring => match event { 0x3f00 => RestoreTick, 0x3f4a => Notify, 0x3f03 => RestoreAbort, 0x3f02 => RestoreStop, 0x3f50 => RestoreParameter, 0x3f06 => RestoreTimer, 0x3f4d => Parameter, 0x3f51 => RestoreResult, _ => return None },
    })
}
