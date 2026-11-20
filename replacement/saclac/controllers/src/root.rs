#![forbid(unsafe_code)]

use crate::lifecycle::{self, Startup};
use xm1_saclac_interface::{CommandKind as K, Message, Parameters, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Context { Root, Lens, Live, S1 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Generation { Current, Previous }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Allocation { Reuse, Replace }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Branch { Lens, Live, S1 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control { ResumeLive, SuspendLive, BeginS1, ReleaseS1, QueryS1, Cancel }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Input<'a> { Message(Message<'a>), Control(Control) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome { Complete, Pending, Inactive, Fault }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Utility {
    RefreshLens,
    PrepareAutofocus,
    CopyAutofocusParameters,
    ReplyAutofocus,
    PublishLensStatus,
    RestoreLensStatus,
    SetLensOption,
    SetLensOptionWithoutWait,
}

pub trait Port: lifecycle::Effects {
    fn startup(&mut self) -> Startup;
    fn initialize_contexts(&mut self);
    fn context(&mut self, context: Context, allocation: Allocation, trigger: Input<'_>) -> RecordHandle;
    fn release(&mut self, context: Context, generation: Generation);
    fn initialize_services(&mut self, record: RecordHandle, parameters: Parameters<'_>);
    fn stop_services(&mut self);
    fn child(&mut self, branch: Branch, input: Input<'_>, record: RecordHandle) -> Outcome;
    fn utility(&mut self, utility: Utility, record: RecordHandle);
    fn autofocus_admitted(&mut self) -> bool;
    fn s1_lens_route(&mut self, record: RecordHandle) -> bool;
    fn still_record(&mut self, record: RecordHandle) -> bool;
    fn live_option_allowed(&mut self) -> bool;
    fn fault(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pending { DuringLensStart, Parameters, Autofocus, LensOperation }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode { Stopped, Started, LensStarting, LensStopping, Live, Pending(Pending), SuspendingForS1, S1 }

pub struct Root {
    mode: Mode,
    release_s1: bool,
}

impl Default for Root { fn default() -> Self { Self::new() } }

impl Root {
    pub const fn new() -> Self {
        Self { mode: Mode::Stopped, release_s1: false }
    }

    fn dispatch(&mut self, branch: Branch, input: Input<'_>, allocation: Allocation, port: &mut impl Port) -> Outcome {
        let context = match branch {
            Branch::Lens => Context::Lens,
            Branch::Live => Context::Live,
            Branch::S1 => Context::S1,
        };
        let record = port.context(context, allocation, input);
        port.child(branch, input, record)
    }

    fn fault(port: &mut impl Port) -> Outcome { port.fault(); Outcome::Fault }

    fn release_context(port: &mut impl Port, context: Context) { port.release(context, Generation::Current); }

    fn forward_live(&mut self, input: Input<'_>, pending: Pending, port: &mut impl Port) -> Outcome {
        match self.dispatch(Branch::Live, input, Allocation::Reuse, port) {
            Outcome::Complete => Outcome::Pending,
            Outcome::Pending => { self.mode = Mode::Pending(pending); Outcome::Pending }
            _ => Self::fault(port),
        }
    }

    pub fn handle(&mut self, message: Message<'_>, port: &mut impl Port) -> Outcome {
        let input = Input::Message(message);
        if matches!(message, Message::Completed(completed) if completed.completion == xm1_saclac_interface::Completion::Lens3c) {
            if matches!(self.mode, Mode::Stopped | Mode::Started) { return Self::fault(port); }
            let record = port.context(Context::Root, Allocation::Reuse, input);
            port.utility(Utility::RestoreLensStatus, record);
            Self::release_context(port, Context::Root);
            return Outcome::Complete;
        }
        if let Message::Command(command) = message {
            match command.kind {
                K::Start if matches!(self.mode, Mode::Stopped | Mode::Started) => {
                    let stopped = self.mode == Mode::Stopped;
                    let startup = port.startup();
                    let resume_live = stopped && startup == Startup::ResumeLive;
                    port.initialize_contexts();
                    let record = port.context(Context::Lens, Allocation::Replace, input);
                    port.initialize_services(record, command.parameters);
                    if resume_live && port.child(Branch::Live, Input::Control(Control::ResumeLive), record) != Outcome::Complete {
                        return Self::fault(port);
                    }
                    port.emit(lifecycle::Request::ReplyStarted);
                    port.release(Context::Lens, Generation::Current);
                    port.release(Context::Lens, Generation::Previous);
                    self.mode = if resume_live { Mode::Live } else { Mode::Started };
                    return Outcome::Pending;
                }
                K::Stop if self.mode == Mode::Started => {
                    self.mode = Mode::Stopped;
                    port.context(Context::Lens, Allocation::Reuse, input);
                    port.stop_services();
                    port.emit(lifecycle::Request::ReplyStopped);
                    for (context, generation) in [
                        (Context::Lens, Generation::Current), (Context::Lens, Generation::Previous),
                        (Context::Live, Generation::Current), (Context::Live, Generation::Previous),
                        (Context::S1, Generation::Current), (Context::S1, Generation::Previous),
                    ] { port.release(context, generation); }
                    return Outcome::Pending;
                }
                K::LensBringUp if matches!(self.mode, Mode::Started | Mode::Live) => {
                    let live = self.mode == Mode::Live;
                    if live && self.dispatch(Branch::Live, input, Allocation::Reuse, port) == Outcome::Fault { return Self::fault(port); }
                    let record = port.context(Context::Lens, Allocation::Reuse, input);
                    if !live { port.utility(Utility::RefreshLens, record); }
                    match port.child(Branch::Lens, input, record) {
                        Outcome::Complete => { self.mode = Mode::Live; Self::release_context(port, Context::Lens); }
                        Outcome::Pending => self.mode = Mode::LensStarting,
                        _ => return Self::fault(port),
                    }
                    return Outcome::Pending;
                }
                K::LensStop if matches!(self.mode, Mode::Started | Mode::Live) => {
                    let live = self.mode == Mode::Live;
                    if live && self.dispatch(Branch::Live, input, Allocation::Reuse, port) == Outcome::Fault { return Self::fault(port); }
                    match self.dispatch(Branch::Lens, input, Allocation::Reuse, port) {
                        Outcome::Complete => {
                            self.mode = Mode::Started;
                            if live { Self::release_context(port, Context::Live); }
                            Self::release_context(port, Context::Lens);
                        }
                        Outcome::Pending => {
                            self.mode = Mode::LensStopping;
                            if live { Self::release_context(port, Context::Live); }
                        }
                        _ => return Self::fault(port),
                    }
                    return Outcome::Pending;
                }
                K::Command25 if !matches!(self.mode, Mode::Stopped | Mode::Started) => {
                    let record = port.context(Context::Root, Allocation::Reuse, input);
                    port.utility(Utility::PublishLensStatus, record);
                    Self::release_context(port, Context::Root);
                    return Outcome::Complete;
                }
                K::Command27 if self.mode == Mode::Started => {
                    let record = port.context(Context::Live, Allocation::Reuse, input);
                    port.utility(Utility::SetLensOption, record);
                    return Outcome::Complete;
                }
                K::Command27 if self.mode == Mode::Live => {
                    let record = port.context(Context::Live, Allocation::Reuse, input);
                    port.utility(Utility::SetLensOptionWithoutWait, record);
                    return Outcome::Complete;
                }
                K::Command02 if self.mode == Mode::LensStarting => return self.forward_live(input, Pending::DuringLensStart, port),
                K::Command02 | K::Command03 | K::Command22 if self.mode == Mode::Live => return self.forward_live(input, Pending::Parameters, port),
                K::Command17 | K::Prepare | K::Command1d | K::Command1e | K::Command1f | K::Command20 | K::Command21 | K::Command24 if self.mode == Mode::Live => return self.forward_live(input, Pending::Autofocus, port),
                K::Command23 if self.mode == Mode::Live => {
                    match self.dispatch(Branch::Live, input, Allocation::Reuse, port) {
                        Outcome::Complete => port.release(Context::Live, Generation::Previous),
                        Outcome::Pending => self.mode = Mode::Pending(Pending::Autofocus),
                        _ => return Self::fault(port),
                    }
                    return Outcome::Pending;
                }
                K::Autofocus if self.mode == Mode::Started => {
                    let record = port.context(Context::Live, Allocation::Reuse, input);
                    port.utility(Utility::CopyAutofocusParameters, record);
                    port.utility(Utility::PrepareAutofocus, record);
                    port.utility(Utility::ReplyAutofocus, record);
                    Self::release_context(port, Context::Live);
                    return Outcome::Pending;
                }
                K::Autofocus if self.mode == Mode::Live => {
                    let record = port.context(Context::Live, Allocation::Replace, input);
                    port.utility(Utility::PrepareAutofocus, record);
                    return match port.child(Branch::Live, input, record) {
                        Outcome::Complete => { port.release(Context::Live, Generation::Previous); Outcome::Pending }
                        Outcome::Pending => { self.mode = Mode::Pending(Pending::Autofocus); Outcome::Pending }
                        _ => Self::fault(port),
                    };
                }
                K::Autofocus if self.mode == Mode::Pending(Pending::LensOperation) => {
                    if !port.autofocus_admitted() { return Self::fault(port); }
                    let record = port.context(Context::Live, Allocation::Reuse, input);
                    port.utility(Utility::PrepareAutofocus, record);
                    port.child(Branch::Live, input, record);
                    return Outcome::Pending;
                }
                K::Autofocus if self.mode == Mode::S1 => {
                    let record = port.context(Context::S1, Allocation::Reuse, input);
                    port.utility(Utility::PrepareAutofocus, record);
                    return match port.child(Branch::S1, input, record) {
                        Outcome::Complete | Outcome::Pending => Outcome::Pending,
                        _ => Self::fault(port),
                    };
                }
                K::HalfPress | K::Command07 | K::Command09 if self.mode == Mode::Live => {
                    self.dispatch(Branch::S1, input, Allocation::Reuse, port);
                    let record = port.context(Context::Live, Allocation::Reuse, input);
                    match port.child(Branch::Live, Input::Control(Control::SuspendLive), record) {
                        Outcome::Complete => {
                            self.mode = Mode::S1;
                            port.child(Branch::S1, Input::Control(Control::BeginS1), record);
                        }
                        Outcome::Pending => self.mode = Mode::SuspendingForS1,
                        _ => return Self::fault(port),
                    }
                    return Outcome::Pending;
                }
                K::Command08 if self.mode == Mode::Live => {
                    self.dispatch(Branch::S1, input, Allocation::Reuse, port);
                    self.mode = Mode::S1;
                    return Outcome::Pending;
                }
                K::Command0c | K::Command28 if self.mode == Mode::Live => {
                    self.dispatch(Branch::S1, input, Allocation::Reuse, port);
                    return Outcome::Pending;
                }
                K::HalfRelease if self.mode == Mode::Live => {
                    self.release_s1 = false;
                    self.dispatch(Branch::Live, input, Allocation::Reuse, port);
                    return Outcome::Pending;
                }
                K::HalfRelease if matches!(self.mode, Mode::SuspendingForS1 | Mode::S1) => {
                    self.release_s1 = true;
                    self.dispatch(Branch::S1, Input::Control(Control::ReleaseS1), Allocation::Reuse, port);
                    return Outcome::Pending;
                }
                K::LensDrive | K::Cancel | K::Command10 | K::Command11 | K::Command13 | K::Command14 | K::Command15 | K::Command16 | K::Command2b | K::Command2c | K::Command2d if self.mode == Mode::Live => return self.forward_live(input, Pending::LensOperation, port),
                K::Prepare26 | K::Command29 if self.mode == Mode::Live => {
                    return match self.dispatch(Branch::Live, input, Allocation::Reuse, port) {
                        Outcome::Complete => { self.mode = Mode::Live; Outcome::Pending }
                        Outcome::Pending => Outcome::Pending,
                        _ => Self::fault(port),
                    };
                }
                K::Command17 | K::Prepare | K::Command1d | K::Command1e | K::Command1f | K::Command20 | K::Command21 | K::Command23 | K::Command11 | K::Prepare26 | K::Command29 if self.mode == Mode::Started => {
                    return match self.dispatch(Branch::Live, input, Allocation::Reuse, port) {
                        Outcome::Complete => { Self::release_context(port, Context::Live); Outcome::Pending }
                        Outcome::Pending => Outcome::Pending,
                        _ => Self::fault(port),
                    };
                }
                K::Command17 | K::Command1d | K::HalfPress | K::Command07 | K::Command0c | K::Command28 | K::Command09 | K::Command0a | K::Command23 | K::Command27 if self.mode == Mode::S1 => {
                    self.dispatch(Branch::S1, input, Allocation::Reuse, port);
                    return Outcome::Pending;
                }
                K::Command02 | K::Command03 | K::Prepare if self.mode == Mode::S1 => {
                    return match self.dispatch(Branch::S1, input, Allocation::Reuse, port) {
                        Outcome::Complete | Outcome::Pending => Outcome::Pending,
                        _ => Self::fault(port),
                    };
                }
                K::HalfPress | K::Command07 | K::Command09 if self.mode == Mode::SuspendingForS1 => {
                    self.dispatch(Branch::S1, input, Allocation::Reuse, port);
                    return Outcome::Pending;
                }
                K::LensDrive if self.mode == Mode::S1 => return self.s1_lens(input, true, port),
                K::Cancel | K::Command10 if self.mode == Mode::S1 => return self.s1_lens(input, false, port),
                K::Command14 | K::Command15 | K::Command16 | K::Command2b | K::Command2c | K::Command2d if self.mode == Mode::S1 => {
                    return match self.dispatch(Branch::Live, input, Allocation::Reuse, port) {
                        Outcome::Complete | Outcome::Pending => Outcome::Pending,
                        _ => Self::fault(port),
                    };
                }
                K::LensDrive | K::Cancel | K::Command10 | K::Command11 | K::Command13 | K::Command14 | K::Command15 | K::Command16 | K::Command2b | K::Command2c | K::Command2d if self.mode == Mode::Pending(Pending::LensOperation) => {
                    return match self.dispatch(Branch::Live, input, Allocation::Reuse, port) {
                        Outcome::Complete => { self.mode = Mode::Live; Outcome::Pending }
                        Outcome::Pending => Outcome::Pending,
                        _ => Self::fault(port),
                    };
                }
                K::Command24 if matches!(self.mode, Mode::Pending(Pending::Parameters) | Mode::Pending(Pending::LensOperation) | Mode::SuspendingForS1 | Mode::S1) => {
                    if !port.live_option_allowed() { return Self::fault(port); }
                    let context = if matches!(self.mode, Mode::SuspendingForS1 | Mode::S1) { Context::S1 } else { Context::Live };
                    let record = port.context(context, Allocation::Reuse, input);
                    port.child(Branch::Live, input, record);
                    return Outcome::Pending;
                }
                _ => return Self::fault(port),
            }
        }
        self.completed(input, port)
    }

    fn s1_lens(&mut self, input: Input<'_>, drive: bool, port: &mut impl Port) -> Outcome {
        let record = port.context(Context::Live, Allocation::Reuse, input);
        let state = port.child(Branch::S1, Input::Control(Control::QueryS1), record);
        let route = port.s1_lens_route(record);
        let use_s1 = state == Outcome::Pending && route;
        let (branch, event) = if use_s1 {
            (Branch::S1, if drive { Input::Control(Control::Cancel) } else { input })
        } else if !drive {
            (Branch::Live, input)
        } else if port.still_record(record) {
            match port.child(Branch::S1, Input::Control(Control::QueryS1), record) {
                Outcome::Complete => (Branch::Live, input),
                Outcome::Pending => (Branch::Live, Input::Control(Control::Cancel)),
                _ => return Self::fault(port),
            }
        } else {
            (Branch::Live, Input::Control(Control::Cancel))
        };
        match port.child(branch, event, record) {
            Outcome::Complete | Outcome::Pending => Outcome::Pending,
            _ => Self::fault(port),
        }
    }

    fn completed(&mut self, input: Input<'_>, port: &mut impl Port) -> Outcome {
        match self.mode {
            Mode::Stopped => Self::fault(port),
            Mode::Started => {
                match self.dispatch(Branch::Live, input, Allocation::Reuse, port) {
                    Outcome::Complete | Outcome::Pending => Outcome::Pending,
                    _ => Self::fault(port),
                }
            }
            Mode::Live | Mode::Pending(Pending::LensOperation) => {
                let pending = self.mode != Mode::Live;
                match self.dispatch(Branch::Live, input, Allocation::Reuse, port) {
                    Outcome::Complete => { if pending { self.mode = Mode::Live; } Outcome::Pending }
                    Outcome::Pending => Outcome::Pending,
                    _ => Self::fault(port),
                }
            }
            Mode::LensStarting => self.join_start(input, port),
            Mode::LensStopping => {
                let outcome = self.dispatch(Branch::Lens, input, Allocation::Reuse, port);
                self.mode = if outcome == Outcome::Complete { Mode::Started } else { Mode::LensStopping };
                if outcome == Outcome::Complete { Self::release_context(port, Context::Lens); }
                Outcome::Pending
            }
            Mode::Pending(Pending::DuringLensStart) => self.join_start(input, port),
            Mode::Pending(Pending::Parameters) => {
                let record = port.context(Context::Live, Allocation::Reuse, input);
                match port.child(Branch::Live, input, record) {
                    Outcome::Complete => {
                        self.mode = Mode::Live;
                        Outcome::Pending
                    }
                    Outcome::Pending => Outcome::Pending,
                    _ => Self::fault(port),
                }
            }
            Mode::Pending(Pending::Autofocus) => {
                match self.dispatch(Branch::Live, input, Allocation::Reuse, port) {
                    Outcome::Complete => {
                        self.mode = Mode::Live;
                        port.release(Context::Live, Generation::Previous);
                        Outcome::Pending
                    }
                    Outcome::Pending => Outcome::Pending,
                    _ => Self::fault(port),
                }
            }
            Mode::SuspendingForS1 => {
                match self.dispatch(Branch::Live, input, Allocation::Reuse, port) {
                    Outcome::Complete => {
                        self.mode = Mode::S1;
                        self.dispatch(Branch::S1, Input::Control(Control::BeginS1), Allocation::Reuse, port);
                        Outcome::Pending
                    }
                    Outcome::Pending => Outcome::Pending,
                    _ => Self::fault(port),
                }
            }
            Mode::S1 => {
                let record = port.context(Context::Live, Allocation::Reuse, input);
                let state = port.child(Branch::S1, Input::Control(Control::QueryS1), record);
                let route = port.s1_lens_route(record);
                let use_s1 = state == Outcome::Pending && route;
                let outcome = if use_s1 {
                    port.child(Branch::S1, input, record)
                } else if live_completion(input) {
                    return match self.dispatch(Branch::Live, input, Allocation::Reuse, port) {
                        Outcome::Fault => Self::fault(port),
                        _ => Outcome::Pending,
                    };
                } else {
                    self.dispatch(Branch::S1, input, Allocation::Reuse, port)
                };
                match outcome {
                    Outcome::Complete => {
                        self.mode = Mode::Live;
                        if self.release_s1 {
                            self.release_s1 = false;
                            Self::release_context(port, Context::S1);
                            self.dispatch(Branch::Live, Input::Control(Control::ReleaseS1), Allocation::Reuse, port);
                        } else { Self::release_context(port, Context::S1); }
                        Outcome::Pending
                    }
                    Outcome::Pending => Outcome::Pending,
                    _ => Self::fault(port),
                }
            }
        }
    }

    fn join_start(&mut self, input: Input<'_>, port: &mut impl Port) -> Outcome {
        let lens = self.dispatch(Branch::Lens, input, Allocation::Reuse, port);
        match lens {
            Outcome::Complete => Self::release_context(port, Context::Lens),
            Outcome::Pending | Outcome::Inactive => {}
            Outcome::Fault => return Self::fault(port),
        }
        let live = self.dispatch(Branch::Live, input, Allocation::Reuse, port);
        if live == Outcome::Fault { return Self::fault(port); }
        if self.mode == Mode::Pending(Pending::DuringLensStart) {
            self.mode = match (lens, live) {
                (Outcome::Complete, Outcome::Complete) => Mode::Live,
                (Outcome::Complete, Outcome::Pending | Outcome::Inactive) => Mode::Pending(Pending::Parameters),
                (Outcome::Pending | Outcome::Inactive, Outcome::Complete) => Mode::LensStarting,
                _ => self.mode,
            };
        } else if lens == Outcome::Complete {
            self.mode = Mode::Live;
        }
        if lens == Outcome::Inactive && live == Outcome::Inactive {
            port.release(Context::Live, Generation::Previous);
        }
        Outcome::Pending
    }
}


fn live_completion(input: Input<'_>) -> bool {
    use xm1_saclac_interface::Completion as C;
    matches!(input, Input::Message(Message::Completed(completed)) if matches!(completed.completion,
        C::Lens33 | C::Lens34 | C::Image43 | C::Lens3d | C::Image44 | C::Statistics82 |
        C::Lens37 | C::Statistics77 | C::Calculation8b | C::Image47 | C::Image66 | C::Lens39))
}

#[cfg(saclac_qualification)]
pub mod qualification;
