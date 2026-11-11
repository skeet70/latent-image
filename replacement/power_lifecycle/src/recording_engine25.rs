//! Recording output and finalization controllers from X-M1 1.50.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordingKind(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordingSettings(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordingMessage(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordingController { Output, Finalize }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordingAction {
    Ready,
    Pending,
    StartOutput,
    Defer,
    StopOutput,
    PublishOutput,
    FinishOutput,
    FinishStop,
    StartFinalize,
    FinishFinalize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordingEngineError { InvalidState { controller: RecordingController, state: u16 } }

pub trait RecordingEngine {
    fn message(&self) -> RecordingMessage;
    fn set_output_request(&mut self, kind: RecordingKind, settings: RecordingSettings);
    fn set_finalize_request(&mut self, kind: RecordingKind);
    fn state(&self, controller: RecordingController) -> u16;
    fn set_state(&mut self, controller: RecordingController, state: u16);
    fn set_row(&mut self, controller: RecordingController, row: u16);
    fn execute(&mut self, action: RecordingAction) -> i32;
}

#[derive(Clone, Copy)]
struct Transition { action: RecordingAction, next: Option<u16> }
use RecordingAction::*;
const fn t(action: RecordingAction, next: Option<u16>) -> Transition { Transition { action, next } }
const OUTPUT: [[Transition; 4]; 10] = [
    [t(Ready,None),t(Pending,None),t(Ready,None),t(Pending,None)],
    [t(StartOutput,Some(1)),t(Defer,None),t(StartOutput,Some(1)),t(Defer,None)],
    [t(StopOutput,Some(3)),t(Defer,None),t(StopOutput,Some(3)),t(Defer,None)],
    [t(Ready,Some(2)),t(Defer,None),t(Defer,None),t(Defer,None)],
    [t(Defer,None),t(Pending,None),t(Defer,None),t(Defer,None)],
    [t(Defer,None),t(PublishOutput,None),t(Defer,None),t(Defer,None)],
    [t(Defer,None),t(FinishOutput,Some(2)),t(Defer,None),t(Defer,None)],
    [t(Defer,None),t(Defer,None),t(Defer,None),t(FinishStop,Some(0))],
    [t(Ready,None),t(Pending,None),t(Ready,None),t(Pending,None)],
    [t(Defer,None),t(Defer,None),t(Defer,None),t(Defer,None)],
];
const FINALIZE: [[Transition; 2]; 5] = [
    [t(Ready,None),t(Pending,None)],
    [t(StartFinalize,Some(1)),t(Defer,None)],
    [t(Defer,None),t(FinishFinalize,Some(0))],
    [t(Ready,None),t(Pending,None)],
    [t(Defer,None),t(Defer,None)],
];

pub fn request_output(engine: &mut impl RecordingEngine, kind: RecordingKind, settings: RecordingSettings) -> Result<i32, RecordingEngineError> {
    engine.set_output_request(kind, settings);
    let row = match kind.0 {
        13 => 0,
        0..=4 => 1,
        6..=11 => 2,
        5 => 3,
        14 => match engine.message().0 { 0x1397 => 4, 0x1398 => 5, 0x1399 => 6, 0x139a => 7, _ => 8 },
        _ => 9,
    };
    engine.set_row(RecordingController::Output, row as u16);
    let state = engine.state(RecordingController::Output);
    let transition = OUTPUT[row].get(usize::from(state)).ok_or(RecordingEngineError::InvalidState { controller: RecordingController::Output, state })?;
    let result = engine.execute(transition.action);
    if let Some(next) = transition.next { engine.set_state(RecordingController::Output, next); }
    Ok(result)
}

pub fn request_finalize(engine: &mut impl RecordingEngine, kind: RecordingKind) -> Result<i32, RecordingEngineError> {
    engine.set_finalize_request(kind);
    let row = match kind.0 { 0 => 0, 2 => 1, 25 if engine.message().0 == 0x1395 => 2, 25 => 3, _ => 4 };
    engine.set_row(RecordingController::Finalize, row as u16);
    let state = engine.state(RecordingController::Finalize);
    let transition = FINALIZE[row].get(usize::from(state)).ok_or(RecordingEngineError::InvalidState { controller: RecordingController::Finalize, state })?;
    let result = engine.execute(transition.action);
    if let Some(next) = transition.next { engine.set_state(RecordingController::Finalize, next); }
    Ok(result)
}
