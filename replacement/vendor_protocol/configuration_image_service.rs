//! X-M1 image-service request CPU. Queue consumers remain separate from request construction.

use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey, CpuReturn};
use core::pin::Pin;
use xm1_power_lifecycle::capture_task3::{self, StackResidue, Task3Queues, Task3Runtime};
use xm1_power_lifecycle::record_address_space::{GuestAddress, GuestAddressSpace, RecordContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotificationCode(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageStateAddress(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageOpenRequest {
    pub input: ImageStateAddress,
    pub configuration: ImageStateAddress,
    pub resources: ImageStateAddress,
    pub option: u32,
    pub notification: NotificationCode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageCloseRequest {
    pub operation: u32,
    pub option: u32,
    pub context: u32,
    pub notification: NotificationCode,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageOutputRequest {
    pub operation: u32,
    pub option: u32,
    pub notification: NotificationCode,
    pub context: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageRequest {
    Open(ImageOpenRequest),
    Close(ImageCloseRequest),
    Output(ImageOutputRequest),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExposureCallbackAddress(pub GuestAddress);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExposureRequest {
    pub callback: ExposureCallbackAddress,
    pub mode: u16,
    pub context: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputMode {
    pub mode_54: u32,
    pub mode_5c: u32,
    pub mode_60: u32,
}

pub struct OutputConfiguration {
    words: [u32; 250],
}

impl OutputConfiguration {
    pub const fn from_abi(words: [u32; 250]) -> Self { Self { words } }
    pub const fn words(&self) -> &[u32; 250] { &self.words }

    pub fn prepare(&mut self, defaults: &Self, mode: OutputMode) {
        self.words.copy_from_slice(&defaults.words);
        self.words[0] = 0x2c;
        self.words[0x54 / 4] = mode.mode_54;
        self.words[0x5c / 4] = mode.mode_5c;
        self.words[0x60 / 4] = mode.mode_60;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommonOutputKeys {
    pub state: ConfigurationKey,
    pub mode: ConfigurationKey,
    pub minutes: ConfigurationKey,
    pub seconds: ConfigurationKey,
    pub active: ConfigurationKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputStep {
    SubmitConfiguration,
    BeginOutput,
    CancelOutput,
    PublishOutput,
    Completed(ActionCompletion),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputError { Busy, UnexpectedPublication, Configuration(BackendError) }
impl From<BackendError> for OutputError {
    fn from(error: BackendError) -> Self { Self::Configuration(error) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Idle, Configuration, Begin, Cancel, Publish }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputPeriodSeconds(pub u32);

pub struct CommonOutputRuntime {
    phase: Phase,
    active: bool,
    period_seconds: OutputPeriodSeconds,
}

impl CommonOutputRuntime {
    pub const fn from_state(active: i32, period_seconds: u32) -> Self {
        Self { phase: Phase::Idle, active: active == 1, period_seconds: OutputPeriodSeconds(period_seconds) }
    }
    pub const fn active(&self) -> bool { self.active }
    pub const fn period_seconds(&self) -> u32 { self.period_seconds.0 }

    pub fn begin(&mut self, keys: CommonOutputKeys, configuration: &mut ConfigImages<'_>, defaults: &OutputConfiguration, modes: &[OutputMode; 7], output: &mut OutputConfiguration) -> Result<OutputStep, OutputError> {
        if self.phase != Phase::Idle { return Err(OutputError::Busy); }
        match keys.state.read(configuration)? {
            0 => {
                keys.active.write(configuration, u8::from(self.active()))?;
                Ok(OutputStep::Completed(ActionCompletion::service_success()))
            }
            1 => {
                let mode = keys.mode.read(configuration)?;
                let minutes = keys.minutes.read(configuration)?;
                let seconds = keys.seconds.read(configuration)?;
                let seconds = if seconds > 60 { 0 } else { seconds };
                if minutes | seconds == 0 {
                    return Ok(OutputStep::Completed(ActionCompletion::service_rejected()));
                }
                self.period_seconds = OutputPeriodSeconds(u32::from(minutes) * 60 + u32::from(seconds));
                output.prepare(defaults, modes[usize::from(if mode > 6 { 0 } else { mode })]);
                self.phase = Phase::Configuration;
                Ok(OutputStep::SubmitConfiguration)
            }
            _ => Ok(OutputStep::Completed(ActionCompletion::service_success())),
        }
    }

    pub fn publication(&mut self, result: CpuReturn) -> Result<OutputStep, OutputError> {
        let rejected = result == CpuReturn::REJECTED;
        let step = match self.phase {
            Phase::Idle | Phase::Publish => return Err(OutputError::UnexpectedPublication),
            Phase::Configuration if rejected => OutputStep::Completed(ActionCompletion::service_rejected()),
            Phase::Configuration => { self.phase = Phase::Begin; return Ok(OutputStep::BeginOutput); }
            Phase::Begin if rejected => { self.phase = Phase::Cancel; return Ok(OutputStep::CancelOutput); }
            Phase::Begin => { self.phase = Phase::Publish; return Ok(OutputStep::PublishOutput); }
            Phase::Cancel => OutputStep::Completed(ActionCompletion::service_rejected()),
        };
        self.phase = Phase::Idle;
        Ok(step)
    }

    pub fn output_published(&mut self) -> Result<ActionCompletion, OutputError> {
        if self.phase != Phase::Publish { return Err(OutputError::UnexpectedPublication); }
        self.active = true;
        self.phase = Phase::Idle;
        Ok(ActionCompletion::service_success())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Progress { Waiting, Completed(ActionCompletion) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceError {
    Busy,
    Idle,
    Cpu(capture_task3::Error),
    Output(OutputError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pending { Idle, Image, Common }

pub struct ImageServiceRuntime<'cpu> {
    cpu: Pin<&'cpu mut Task3Runtime>,
    common: CommonOutputRuntime,
    pending: Pending,
    cancel_residue: StackResidue,
}

impl<'cpu> ImageServiceRuntime<'cpu> {
    pub fn new(cpu: Pin<&'cpu mut Task3Runtime>, active: i32, period_seconds: u32) -> Self {
        Self { cpu, common: CommonOutputRuntime::from_state(active, period_seconds), pending: Pending::Idle, cancel_residue: StackResidue(0) }
    }

    pub const fn active(&self) -> bool { self.common.active() }
    pub const fn period_seconds(&self) -> u32 { self.common.period_seconds() }

    pub fn begin_image<M: GuestAddressSpace, E: Task3Queues>(&mut self, context: &mut RecordContext<M, E>, request: ImageRequest) -> Result<Progress, ServiceError> {
        if self.pending != Pending::Idle { return Err(ServiceError::Busy); }
        let request = match request {
            ImageRequest::Open(r) => capture_task3::Request::Open {
                input: capture_task3::ImageAddress(GuestAddress::new(r.input.0)),
                configuration: capture_task3::ImageAddress(GuestAddress::new(r.configuration.0)),
                resources: capture_task3::ImageAddress(GuestAddress::new(r.resources.0)),
                option: r.option, notification: capture_task3::NotificationCode(r.notification.0),
            },
            ImageRequest::Close(r) => capture_task3::Request::Close {
                operation: r.operation, option: r.option, context: r.context,
                notification: capture_task3::NotificationCode(r.notification.0), reserved: r.reserved,
            },
            ImageRequest::Output(r) => capture_task3::Request::Output {
                operation: r.operation, option: r.option,
                notification: capture_task3::NotificationCode(r.notification.0), context: r.context,
            },
        };
        self.cpu.as_mut().submit(context, request).map_err(ServiceError::Cpu)?;
        self.pending = Pending::Image;
        Ok(Progress::Waiting)
    }

    pub fn submit_exposure<M: GuestAddressSpace, E: Task3Queues>(&mut self, context: &mut RecordContext<M, E>, request: ExposureRequest, residue: StackResidue) -> Result<(), ServiceError> {
        if self.pending != Pending::Idle { return Err(ServiceError::Busy); }
        self.cpu.as_mut().submit(context, capture_task3::Request::Exposure {
            value: request.callback.0.get(), mode: request.mode, context: request.context, residue,
        }).map_err(ServiceError::Cpu)
    }

    pub fn begin_common<M: GuestAddressSpace, E: Task3Queues>(&mut self, context: &mut RecordContext<M, E>, keys: CommonOutputKeys, configuration: &mut ConfigImages<'_>, defaults: &OutputConfiguration, modes: &[OutputMode; 7], output: &mut OutputConfiguration, cancel_residue: StackResidue) -> Result<Progress, ServiceError> {
        if self.pending != Pending::Idle { return Err(ServiceError::Busy); }
        match self.common.begin(keys, configuration, defaults, modes, output).map_err(ServiceError::Output)? {
            OutputStep::Completed(completion) => Ok(Progress::Completed(completion)),
            OutputStep::SubmitConfiguration => {
                self.cancel_residue = cancel_residue;
                self.post_common(context, capture_task3::Request::Configure {
                    configuration: &mut output.words, notification: capture_task3::NotificationCode(0),
                })?;
                self.pending = Pending::Common;
                Ok(Progress::Waiting)
            }
            _ => Err(ServiceError::Output(OutputError::UnexpectedPublication)),
        }
    }

    // dc38d4 ignores enqueue results and waits for queue7 publications.
    fn post_common<M: GuestAddressSpace, E: Task3Queues>(&mut self, context: &mut RecordContext<M, E>, request: capture_task3::Request<'_>) -> Result<(), ServiceError> {
        match self.cpu.as_mut().submit(context, request) {
            Ok(()) | Err(capture_task3::Error::Transport(_)) => Ok(()),
            Err(error) => Err(ServiceError::Cpu(error)),
        }
    }

    pub fn poll<M: GuestAddressSpace, E: Task3Queues>(&mut self, context: &mut RecordContext<M, E>) -> Result<Progress, ServiceError> {
        if self.pending == Pending::Idle { return Err(ServiceError::Idle); }
        let Some(result) = self.cpu.as_mut().poll(context).map_err(ServiceError::Cpu)? else {
            return Ok(Progress::Waiting);
        };
        if self.pending != Pending::Common {
            self.pending = Pending::Idle;
            return Ok(Progress::Completed(if result == -255 { ActionCompletion::service_rejected() } else { ActionCompletion::service_success() }));
        }
        match self.common.publication(CpuReturn::from_abi(result)).map_err(ServiceError::Output)? {
            OutputStep::BeginOutput => {
                self.post_common(context, capture_task3::Request::Begin {
                    operation: 0x9f, option: 0, notification: capture_task3::NotificationCode(0), context: 0,
                })?;
                Ok(Progress::Waiting)
            }
            OutputStep::CancelOutput => {
                self.post_common(context, capture_task3::Request::Cancel {
                    operation: 0, option: 0x15, residue: self.cancel_residue,
                })?;
                Ok(Progress::Waiting)
            }
            OutputStep::PublishOutput => {
                self.post_common(context, capture_task3::Request::Output {
                    operation: 0x9f, option: 0, notification: capture_task3::NotificationCode(0), context: 0,
                })?;
                self.pending = Pending::Idle;
                self.common.output_published().map(Progress::Completed).map_err(ServiceError::Output)
            }
            OutputStep::Completed(completion) => {
                self.pending = Pending::Idle;
                Ok(Progress::Completed(completion))
            }
            OutputStep::SubmitConfiguration => Err(ServiceError::Output(OutputError::UnexpectedPublication)),
        }
    }
}

#[cfg(test)]
#[path = "configuration_image_service_tests.rs"]
mod tests;
