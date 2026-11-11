use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_high::{CallbackState, lens::{Error as LensError, LensAction, LensActions, TransferQuery}};
use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey};
use crate::configuration_actions_middle::{MiddleAction, MiddleArguments, property::{MiddlePropertyActions, PropertyError, PropertyProgress}};
use crate::configuration_diagnostic_actor::{DiagnosticActor, DiagnosticError, DiagnosticOutcome};
use crate::configuration_diagnostic_registers::DiagnosticTick;
use crate::configuration_i2c_actor::{RegisterIo, TransferId};
use crate::configuration_serial_actors::{PeripheralFlags, PeripheralStatusBytes, SerialConfiguration, SerialError, SerialStart};
use crate::configuration_status_actor::{StatusActor, StatusError, StatusStart};
use crate::lens_queue_runtime::{LensQueueRuntime, Publication, packets::CallbackId};
use xm1_lens_transport::{control_actor::ControlActor, wire::{Error as WireError, InterruptController, Mmio, Platform}};

#[derive(Clone, Copy)]
pub enum Request<'a> {
    Serial14(&'a [ConfigurationKey; 14]),
    Diagnostic16 { keys: &'a [ConfigurationKey; 2], now: DiagnosticTick },
    ImagingImmediate(crate::configuration_actions_imaging::ImmediateAction),
    HighImmediate(crate::configuration_actions_high::ImmediateAction),
    Status17([ConfigurationKey; 8]),
    Middle { action: MiddleAction, arguments: MiddleArguments<'a> },
    Lens(LensAction),
    Transfer(TransferQuery),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Progress { Waiting, Completed(ActionCompletion) }

#[derive(Debug, Eq, PartialEq)]
pub enum Error<E> {
    Busy,
    Configuration(BackendError),
    Serial(SerialError),
    Diagnostic(DiagnosticError),
    Status(StatusError),
    Property(PropertyError<E>),
    Lens(LensError<E>),
}

#[derive(Clone, Copy)]
enum Pending { Serial(TransferId), Diagnostic, Status, Property, Lens }

pub struct ConfigurationRuntime {
    serial: SerialConfiguration,
    diagnostic: DiagnosticActor,
    status: StatusActor,
    property: MiddlePropertyActions,
    lens: LensActions,
    pending: Option<Pending>,
}

impl ConfigurationRuntime {
    pub fn adopt(status: PeripheralStatusBytes, flags: PeripheralFlags, callback: CallbackId) -> Self {
        Self { serial: SerialConfiguration::new(), diagnostic: DiagnosticActor::new(), status: StatusActor::adopt(status, flags, callback), property: MiddlePropertyActions::default(), lens: LensActions::new(), pending: None }
    }

    pub fn low(
        &mut self,
        action: crate::configuration_actions_low::LowAction,
        images: &mut ConfigImages<'_>,
        bus: &mut impl crate::configuration_actions_low::PhysicalBus,
        kernel: &mut impl xm1_power_services::osal::CriticalKernel,
        frame: crate::configuration_actions_low::PeripheralFrameResidue,
    ) -> Result<Progress, Error<core::convert::Infallible>> {
        if self.pending.is_some() { return Err(Error::Busy); }
        crate::configuration_actions_low::execute(action, images, bus, kernel, frame)
            .map(Progress::Completed)
            .map_err(Error::Configuration)
    }

    pub fn plane_samples(
        &mut self,
        images: &mut ConfigImages<'_>,
        planes: crate::configuration_actions_imaging::planes::RecordPlanes<'_>,
    ) -> Result<Progress, Error<core::convert::Infallible>> {
        if self.pending.is_some() { return Err(Error::Busy); }
        crate::configuration_actions_imaging::planes::query_plane_samples(images, planes)
            .map(Progress::Completed)
            .map_err(Error::Configuration)
    }

    pub fn begin<M: Mmio, P: Platform, I: InterruptController>(
        &mut self,
        request: Request<'_>,
        images: &mut ConfigImages<'_>,
        io: &mut impl RegisterIo,
        device: &ControlActor,
        callbacks: &mut CallbackState,
        queue: &mut LensQueueRuntime<M, P, I>,
    ) -> Result<Progress, Error<WireError<P::Error, I::Error>>> {
        if self.pending.is_some() { return Err(Error::Busy); }
        let (pending, completion) = match request {
            Request::Serial14(keys) => match self.serial.action14(io, images, keys).map_err(Error::Serial)? {
                SerialStart::Complete(value) => (None, Some(value)),
                SerialStart::Pending(id) => (Some(Pending::Serial(id)), None),
            },
            Request::Diagnostic16 { keys, now } => match self.diagnostic.begin(io, images, keys, now).map_err(Error::Diagnostic)? {
                DiagnosticOutcome::Complete(value) => (None, Some(value)),
                DiagnosticOutcome::Pending => (Some(Pending::Diagnostic), None),
            },
            Request::Status17(keys) => match self.status.begin(queue, keys).map_err(Error::Status)? {
                StatusStart::Complete(value) => (None, Some(value)),
                StatusStart::Pending => (Some(Pending::Status), None),
            },
            Request::ImagingImmediate(action) => (None, Some(action.execute())),
            Request::HighImmediate(action) => (None, Some(action.completion())),
            Request::Middle { action, arguments } => {
                if let Some(result) = crate::configuration_actions_middle::execute_immediate(action, arguments, images) {
                    (None, Some(result.map_err(Error::Configuration)?))
                } else {
                    match self.property.begin(action, arguments, images, device, queue).map_err(Error::Property)? {
                        PropertyProgress::Completed(value) => (None, Some(value)),
                        PropertyProgress::Waiting => (Some(Pending::Property), None),
                    }
                }
            }
            Request::Lens(action) => {
                let result = self.lens.begin(action, images, queue).map_err(Error::Lens)?;
                (result.is_none().then_some(Pending::Lens), result)
            }
            Request::Transfer(query) => {
                let result = self.lens.begin_query(query, images, callbacks, queue).map_err(Error::Lens)?;
                (result.is_none().then_some(Pending::Lens), result)
            }
        };
        self.pending = pending;
        Ok(completion.map_or(Progress::Waiting, Progress::Completed))
    }

    pub fn interrupt(&mut self) -> Result<(), SerialError> {
        match self.pending {
            Some(Pending::Serial(id)) => self.serial.interrupt(id),
            _ => Err(SerialError::Pending),
        }
    }

    pub fn poll<M: Mmio, P: Platform, I: InterruptController>(
        &mut self,
        images: &mut ConfigImages<'_>,
        io: &mut impl RegisterIo,
        now: DiagnosticTick,
        queue: &mut LensQueueRuntime<M, P, I>,
    ) -> Result<Option<ActionCompletion>, Error<WireError<P::Error, I::Error>>> {
        let completion = match self.pending {
            Some(Pending::Serial(id)) => self.serial.poll(io, images, id).map_err(Error::Serial)?,
            Some(Pending::Diagnostic) => match self.diagnostic.poll(io, queue, images, now).map_err(Error::Diagnostic)? {
                DiagnosticOutcome::Complete(value) => Some(value),
                DiagnosticOutcome::Pending => None,
            },
            _ => None,
        };
        if completion.is_some() { self.pending = None; }
        Ok(completion)
    }

    pub fn publication<M: Mmio, P: Platform, I: InterruptController>(
        &mut self,
        publication: Publication,
        images: &mut ConfigImages<'_>,
        device: &ControlActor,
        callbacks: &mut CallbackState,
        completion_saved_value: u32,
        now: DiagnosticTick,
        queue: &mut LensQueueRuntime<M, P, I>,
    ) -> Result<Option<ActionCompletion>, Error<WireError<P::Error, I::Error>>> {
        let completion = match self.pending {
            Some(Pending::Status) => self.status.consume(images, publication, completion_saved_value).map_err(Error::Status)?,
            Some(Pending::Diagnostic) => match self.diagnostic.consume(queue, images, publication, now).map_err(Error::Diagnostic)? {
                Some(DiagnosticOutcome::Complete(value)) => Some(value),
                _ => None,
            },
            Some(Pending::Property) => match self.property.publication(publication, images, device, queue).map_err(Error::Property)? {
                Some(PropertyProgress::Completed(value)) => Some(value),
                _ => None,
            },
            Some(Pending::Lens) => self.lens.publication(publication, images, callbacks, queue).map_err(Error::Lens)?,
            _ => None,
        };
        if completion.is_some() { self.pending = None; }
        Ok(completion)
    }
}

#[cfg(test)]
#[path = "configuration_runtime_tests.rs"]
mod tests;
