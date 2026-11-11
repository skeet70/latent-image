//! Task 3 recording continuation and its controller 15 completion mask.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Field {
    Selector = 0x310c,
    SavedCallback = 0x3114,
    Request = 0x5f00,
    ActiveSelector = 0x5f08,
    Callback = 0x5f10,
    OutputKind = 0x5f14,
    Required = 0x5f18,
    Started = 0x5f1c,
    Done = 0x5f20,
    CaptureReady = 0x81d8,
    OutputActive = 0x93b4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum Controller { Output = 4, Continuation = 15 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum OutputKind { Close = 4, Idle = 6, Third = 7, Snapshot = 8, Second = 11, Poll = 14 }

impl OutputKind {
    fn from_word(word: u32) -> Self {
        match word { 6 => Self::Idle, 7 => Self::Third, 11 => Self::Second, _ => Self::Snapshot }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepRequest { Start, Poll }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureRequest { Start, Poll }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FinalizeRequest { Start, Poll }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuxiliaryRequest { StartFirst, StartSecond }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackAddress(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidControllerState { pub controller: Controller, pub state: u16 }

pub trait Runtime {
    fn word(&self, field: Field) -> u32;
    fn set_word(&mut self, field: Field, value: u32);
    fn state(&self, controller: Controller) -> u16;
    fn set_state(&mut self, controller: Controller, state: u16);
    fn set_event(&mut self, controller: Controller, event: u16);
    fn current_operation(&self) -> u16;
    fn defer_current(&mut self);
    fn channel_enabled(&mut self, enabled: bool);
    fn save_current_request(&mut self);
    fn clear_saved_request(&mut self);
    fn step_request(&mut self, request: StepRequest) -> i32;
    fn capture_request(&mut self, request: CaptureRequest) -> i32;
    fn output_request(&mut self, kind: OutputKind) -> i32;
    fn finalize_request(&mut self, request: FinalizeRequest) -> i32;
    fn auxiliary_request(&mut self, request: AuxiliaryRequest) -> i32;
    fn auxiliary_state(&mut self) -> u32;
    fn finish_auxiliary(&mut self) -> i32;
    fn request_service_stop(&mut self);
    fn callback(&mut self, callback: CallbackAddress);
}

fn add(runtime: &mut impl Runtime, field: Field, bits: u32) {
    runtime.set_word(field, runtime.word(field) | bits);
}

fn start_remaining(runtime: &mut impl Runtime) {
    let mut status = 1;
    for bit in [1, 2, 0x40, 4, 8, 0x80, 0x10, 0x20] {
        if runtime.word(Field::Required) & bit == 0 || runtime.word(Field::Started) & bit != 0 || status != 1 { continue; }
        status = match bit {
            1 => runtime.step_request(StepRequest::Start),
            2 => runtime.capture_request(CaptureRequest::Start),
            0x40 => runtime.auxiliary_request(AuxiliaryRequest::StartFirst),
            4 => runtime.output_request(OutputKind::Close),
            8 => runtime.output_request(OutputKind::from_word(runtime.word(Field::OutputKind))),
            0x80 => runtime.auxiliary_request(AuxiliaryRequest::StartSecond),
            0x10 => runtime.finalize_request(FinalizeRequest::Start),
            _ => { runtime.request_service_stop(); 0 },
        };
        add(runtime, Field::Started, bit);
        if bit != 0x20 && status == 1 { add(runtime, Field::Done, bit); }
    }
}

fn finish_if_covered(runtime: &mut impl Runtime) -> i32 {
    if (runtime.word(Field::Required) as u8 as u32) & !runtime.word(Field::Done) != 0 {
        runtime.set_state(Controller::Continuation, 1);
        return 0;
    }
    runtime.set_state(Controller::Continuation, 0);
    runtime.callback(CallbackAddress(runtime.word(Field::Callback)));
    runtime.clear_saved_request();
    runtime.channel_enabled(true);
    1
}

fn begin(runtime: &mut impl Runtime) -> i32 {
    runtime.channel_enabled(false);
    runtime.save_current_request();
    for field in [Field::Required, Field::Started, Field::Done] { runtime.set_word(field, 0); }
    let selector = runtime.word(Field::Selector);
    runtime.set_word(Field::ActiveSelector, selector);
    runtime.set_word(Field::Callback, runtime.word(Field::SavedCallback));
    let output = match selector { 0 => OutputKind::Idle, 1 => OutputKind::Snapshot, 2 => OutputKind::Second, 3 => OutputKind::Third, _ => OutputKind::Snapshot };
    runtime.set_word(Field::OutputKind, output as u32);
    match selector { 0 => runtime.set_word(Field::OutputActive, 0), 1..=3 => runtime.set_word(Field::OutputActive, 1), _ => {} }
    if selector != 0 {
        let capture = if runtime.word(Field::CaptureReady) != 1 { 2 } else { 0 };
        runtime.set_word(Field::Required, 1 | capture | if selector == 2 { 0x1c } else { 0x38 });
    }
    start_remaining(runtime);
    finish_if_covered(runtime)
}

/// Processes command 0x13e7 after its payload becomes the current task 3 message.
pub fn publish_status4(runtime: &mut impl Runtime) -> Result<(), InvalidControllerState> {
    runtime.set_event(Controller::Output, 3);
    match runtime.state(Controller::Output) {
        0 => {
            runtime.set_word(Field::Request, 1);
            runtime.set_event(Controller::Continuation, 1);
            match runtime.state(Controller::Continuation) {
                0 => { begin(runtime); }
                1 => runtime.defer_current(),
                state => return Err(InvalidControllerState { controller: Controller::Continuation, state }),
            }
            runtime.set_state(Controller::Output, 5);
        }
        1..=6 => runtime.defer_current(),
        state => return Err(InvalidControllerState { controller: Controller::Output, state }),
    }
    Ok(())
}

fn event(runtime: &impl Runtime) -> u16 {
    match runtime.word(Field::Request) {
        5 => 0,
        1 => 1,
        6 => match runtime.current_operation() {
            0x13e6 => 2, 0x1394 => 3, 0x1399 => 4, 0x139a => 5,
            0x1395 => 6, 0x13ea => 7, 0x13c9 => 8, 0x13c8 => 9, _ => 10,
        },
        _ => 12,
    }
}

fn dispatch_event(runtime: &mut impl Runtime, event: u16) -> Result<i32, InvalidControllerState> {
    runtime.set_event(Controller::Continuation, event);
    let state = runtime.state(Controller::Continuation);
    if state > 1 { return Err(InvalidControllerState { controller: Controller::Continuation, state }); }
    if state == 0 {
        return Ok(match event { 1 => begin(runtime), 12 => { runtime.defer_current(); 0 }, _ => 1 });
    }
    let result = match event {
        0 | 10 => 0,
        1 | 12 => { runtime.defer_current(); 0 }
        2 => { if runtime.step_request(StepRequest::Poll) == 1 { add(runtime, Field::Done, 1); } 0 }
        3 => { if runtime.capture_request(CaptureRequest::Poll) == 1 { add(runtime, Field::Done, 2); } 0 }
        4 | 5 => { if runtime.output_request(OutputKind::Poll) == 1 { add(runtime, Field::Done, if event == 4 { 4 } else { 8 }); } 0 }
        6 => { if runtime.finalize_request(FinalizeRequest::Poll) == 1 { add(runtime, Field::Done, 0x10); } 0 }
        7 => { add(runtime, Field::Done, 0x20); 0 }
        8 => {
            if runtime.auxiliary_state() == 3 {
                if runtime.finish_auxiliary() == 1 { add(runtime, Field::Done, 0x40); }
                else { runtime.defer_current(); }
            }
            0
        }
        9 => { if runtime.auxiliary_state() == 0 { add(runtime, Field::Done, 0x80); } 0 }
        _ => {
            if (runtime.word(Field::Started) as u8 as u32) & !runtime.word(Field::Done) == 0 {
                start_remaining(runtime);
                finish_if_covered(runtime)
            } else { runtime.set_state(Controller::Continuation, 1); 0 }
        }
    };
    Ok(result)
}

/// Processes controller 15's operation event and its subsequent coverage event.
pub fn continue_status(runtime: &mut impl Runtime) -> Result<i32, InvalidControllerState> {
    let selected = event(runtime);
    let result = dispatch_event(runtime, selected)?;
    if runtime.word(Field::Request) == 6 {
        dispatch_event(runtime, 11)
    } else {
        Ok(result)
    }
}

#[cfg(test)]
#[path = "recording_continuation_tests.rs"]
mod tests;
