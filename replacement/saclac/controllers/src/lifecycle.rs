#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Startup {
    Normal,
    ResumeLive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Service {
    Lens,
    Image,
    Calculation,
    AuxiliaryGate,
    Statistics,
    RecordingBuffers,
    Tracking,
    MotionDetection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    ReserveAccessor,
    ResetImageBuffer,
    Start(Service),
    Stop(Service),
    PublishParameters,
    ReplyStarted,
    ReplyStopped,
}

pub trait Effects {
    fn emit(&mut self, request: Request);
}

/// The ordered service boundary used by both the standalone lifecycle and the root router.
pub fn start_services(startup: Startup, effects: &mut impl Effects) {
    effects.emit(Request::ReserveAccessor);
    if startup == Startup::Normal {
        effects.emit(Request::Start(Service::Lens));
    }
    effects.emit(Request::Start(Service::RecordingBuffers));
    effects.emit(Request::ResetImageBuffer);
    for service in [Service::Image, Service::Calculation, Service::AuxiliaryGate,
        Service::Statistics, Service::Tracking, Service::MotionDetection] {
        effects.emit(Request::Start(service));
    }
    effects.emit(Request::PublishParameters);
}

pub fn stop_services(effects: &mut impl Effects) {
    for service in [Service::Image, Service::Calculation, Service::AuxiliaryGate,
        Service::Statistics, Service::Lens, Service::Tracking, Service::MotionDetection,
        Service::RecordingBuffers] {
        effects.emit(Request::Stop(service));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    Start(Startup),
    Stop,
    LensReady,
    LensStopped,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Started,
    Live,
    Stopped,
    InvalidEvent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Stopped,
    Started,
    Live,
}

pub struct Lifecycle {
    mode: Mode,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl Lifecycle {
    pub const fn new() -> Self {
        Self { mode: Mode::Stopped }
    }

    pub fn handle(&mut self, event: Event, effects: &mut impl Effects) -> Outcome {
        match (self.mode, event) {
            (previous @ (Mode::Stopped | Mode::Started), Event::Start(startup)) => {
                start_services(startup, effects);
                effects.emit(Request::ReplyStarted);
                if previous == Mode::Stopped && startup == Startup::ResumeLive {
                    self.mode = Mode::Live;
                    Outcome::Live
                } else {
                    self.mode = Mode::Started;
                    Outcome::Started
                }
            }
            (Mode::Started, Event::Stop) => {
                stop_services(effects);
                effects.emit(Request::ReplyStopped);
                self.mode = Mode::Stopped;
                Outcome::Stopped
            }
            (Mode::Started, Event::LensReady) => {
                self.mode = Mode::Live;
                Outcome::Live
            }
            (Mode::Live, Event::LensStopped) => {
                self.mode = Mode::Started;
                Outcome::Started
            }
            _ => Outcome::InvalidEvent,
        }
    }
}
