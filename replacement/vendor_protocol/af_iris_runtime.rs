//! X-M1 1.50 task50 iris requests and lower actuator completion ownership.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::af_capture_actor::ClientId;
use crate::af_motion_runtime::{AxisMotion, IrisPosition, MotionContext, ServiceOperation};
use crate::backend::{BackendError, ConfigImages};
use crate::af_position_geometry::PositionGeometry;
use crate::eeprom_detection::{DetectionIo, DetectionProvider, ServiceStatus};
use crate::lens_actuator_cpu::{self, ActuatorCompletion, ActuatorError, ActuatorEvents, ActuatorOperation, ActuatorRequest, IrisOptions, LensActuatorCpu, LensPosition, LensRate, ObserverCompletion, ObserverError, OwnerToken, Selector};
use crate::lens_queue_runtime::LensQueueRuntime;
use xm1_calibration_core::{Calibration, CurveIndex};
use xm1_calibration_core::configuration::CfgKey;
use crate::metadata_calibration::RegionError;
use xm1_lens_transport::control_runtime::Task99Client;
use xm1_lens_transport::control_device::PeriodicIndex;
use xm1_lens_transport::control_types::{CallbackToken, ControlError, PeriodicModes, TickCount};
use xm1_power_lifecycle::record_address_space::{CallbackId as RegisteredCallbackId, CallbackSignature};
use xm1_lens_transport::wire::{Error as WireError, InterruptController, Mmio, Platform};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrisSource(u32);
impl IrisSource {
    pub const fn from_abi(value: u32) -> Self { Self(value) }
    pub const fn abi(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrisMode(u32);
impl IrisMode {
    pub const fn from_abi(value: u32) -> Self { Self(value) }
    pub const fn abi(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrisPolicy(u32);
impl IrisPolicy {
    pub const fn from_abi(value: u32) -> Self { Self(value) }
    pub const fn abi(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IrisOrigin { Standard4213, Direct421d }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrisMoveRequest {
    pub position: IrisPosition,
    pub mode: IrisMode,
    pub policy: IrisPolicy,
    pub source: IrisSource,
    pub origin: IrisOrigin,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IrisPublicationKind { Move4241, Home423f }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrisCompletion {
    pub client: ClientId,
    pub publication: IrisPublicationKind,
    pub service_effect: Option<ServiceOperation>,
}
impl IrisCompletion {
    pub fn publish_service_effect(self, configuration: &mut ConfigImages<'_>) -> Result<(), BackendError> {
        if let Some(operation) = self.service_effect { operation.report_failure(configuration)?; }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IrisStart { Waiting, Duplicate, Completed(IrisCompletion) }
#[derive(Debug, Eq, PartialEq)]
pub enum IrisError<E> {
    Actuator(ActuatorError<E>), Observer(ObserverError<E>), Detection(ServiceStatus),
    Calibration(RegionError), Task99(ControlError),
    UnexpectedReply, MissingTimerRegistration, CallbackSignature,
}
#[derive(Clone, Copy)]
enum Request { Move(IrisMoveRequest), Home }
impl Request {
    const fn publication(self) -> IrisPublicationKind {
        match self { Self::Move(_) => IrisPublicationKind::Move4241, Self::Home => IrisPublicationKind::Home423f }
    }
    const fn operation(self) -> ActuatorOperation {
        match self { Self::Move(_) => ActuatorOperation::IrisMove, Self::Home => ActuatorOperation::Iris }
    }
}
#[derive(Clone, Copy)]
struct Pending {
    client: ClientId, owner: OwnerToken, request: Request,
    control: Option<bool>, command: Option<bool>, observed: bool,
    controls_required: bool,
    timer: Option<RegisteredCallbackId>, timer_armed: bool,
}
impl Pending {
    fn completed(self) -> bool {
        let control_ready = !self.controls_required || self.control.is_some();
        control_ready && self.command.is_some() &&
            (self.control == Some(true) || self.command == Some(true) || self.observed)
    }
}

pub struct IrisRuntime {
    pending: Option<Pending>,
    timer_registration: Option<RegisteredCallbackId>,
}
impl Default for IrisRuntime { fn default() -> Self { Self::new() } }
impl IrisRuntime {
    pub const fn new() -> Self {
        Self { pending: None, timer_registration: None }
    }
    pub const fn is_pending(&self) -> bool { self.pending.is_some() }
    pub fn register_timer(&mut self, callback: RegisteredCallbackId) -> Result<(), IrisError<core::convert::Infallible>> {
        if self.pending.is_some() { return Err(IrisError::UnexpectedReply); }
        if callback.signature() != CallbackSignature::Completion { return Err(IrisError::CallbackSignature); }
        self.timer_registration = Some(callback);
        Ok(())
    }

    pub fn begin_iris_move<M: Mmio, P: Platform, I: InterruptController, B: DetectionIo>(
        &mut self, request: IrisMoveRequest, client: ClientId, context: &mut MotionContext,
        calibration: &mut Calibration<'_>, actuator: &mut LensActuatorCpu,
        queue: &mut LensQueueRuntime<M, P, I>, task99: &Task99Client<'_>,
        detection: &DetectionProvider<'_, B>, events: &ActuatorEvents,
    ) -> Result<IrisStart, IrisError<WireError<P::Error, I::Error>>> {
        if self.pending.is_some() { return Ok(IrisStart::Duplicate); }
        let mut completion = IrisCompletion { client, publication: IrisPublicationKind::Move4241, service_effect: None };
        if context.iris_in_progress {
            if context.service == Some(ServiceOperation::Request70) && request.source.0 == 10 { completion.service_effect = context.service; }
            return Ok(IrisStart::Completed(completion));
        }
        if lens_actuator_cpu::not_suspended(task99) { return Ok(IrisStart::Completed(completion)); }
        let encoded = LensPosition(PositionGeometry::new(calibration).iris_position(CurveIndex(request.position.0 as u32)).map_err(IrisError::Calibration)?);
        let current = LensPosition(PositionGeometry::new(calibration).iris_position(CurveIndex(context.current_iris.0 as u32)).map_err(IrisError::Calibration)?);
        let same_policy = match request.policy.0 {
            0 => context.iris_activity == AxisMotion::Idle,
            1 => context.iris_activity != AxisMotion::Idle,
            _ => false,
        };
        if current == encoded && same_policy {
            let mask = calibration.configuration_mut().read_byte(CfgKey(0x3e2740u32.wrapping_add(request.position.0 as u32))).map_err(IrisError::Calibration)?;
            if mask & 3 != 3 || request.mode.0 != 1 || context.iris_irq_mode {
                return Ok(IrisStart::Completed(completion));
            }
        }
        let use_fast_irq = context.iris_fast_irq && request.source.0 != 1;
        let controls_required = !use_fast_irq || context.iris_source.abi() == 1;
        let selector = if use_fast_irq { 2 } else { 4 };
        if use_fast_irq && self.timer_registration.is_none() { return Err(IrisError::MissingTimerRegistration); }
        self.start(Request::Move(request), client, controls_required, selector, use_fast_irq, actuator, queue, detection, events)?;
        context.iris_source = request.source;
        context.iris_in_progress = true;
        actuator.execute(queue, self.pending.map(|pending| pending.owner), ActuatorRequest::IrisMove {
            position: LensPosition(0), rate: LensRate(encoded.0),
            options: IrisOptions { first: false, second: request.mode.0 == 1, third: request.mode.0 == 1, fourth: request.policy.0 != 1 },
        }).map_err(IrisError::Actuator)?;
        Ok(IrisStart::Waiting)
    }

    pub fn begin_iris_home<M: Mmio, P: Platform, I: InterruptController, B: DetectionIo>(
        &mut self, mode: IrisMode, client: ClientId, context: &mut MotionContext,
        actuator: &mut LensActuatorCpu, queue: &mut LensQueueRuntime<M, P, I>,
        task99: &Task99Client<'_>, detection: &DetectionProvider<'_, B>, events: &ActuatorEvents,
    ) -> Result<IrisStart, IrisError<WireError<P::Error, I::Error>>> {
        if self.pending.is_some() { return Ok(IrisStart::Duplicate); }
        if context.iris_in_progress || lens_actuator_cpu::not_suspended(task99) {
            let service_effect = if context.iris_in_progress && mode.0 == 3 && context.service == Some(ServiceOperation::Request71) { context.service } else { None };
            return Ok(IrisStart::Completed(IrisCompletion { client, publication: IrisPublicationKind::Home423f, service_effect }));
        }
        self.start(Request::Home, client, true, 4, false, actuator, queue, detection, events)?;
        context.iris_in_progress = true;
        actuator.iris(queue, self.pending.map(|pending| pending.owner)).map_err(IrisError::Actuator)?;
        Ok(IrisStart::Waiting)
    }

    fn start<M: Mmio, P: Platform, I: InterruptController, B: DetectionIo>(
        &mut self, request: Request, client: ClientId, controls_required: bool, selector: u8, timer_wait: bool,
        actuator: &mut LensActuatorCpu, queue: &mut LensQueueRuntime<M, P, I>,
        detection: &DetectionProvider<'_, B>, events: &ActuatorEvents,
    ) -> Result<(), IrisError<WireError<P::Error, I::Error>>> {
        let owner = OwnerToken(queue.allocate_callback().map_err(|error| IrisError::Actuator(ActuatorError::Queue(error)))?.0);
        if controls_required { actuator.set_detection_mode(detection, false).map_err(IrisError::Detection)?; }
        if !timer_wait { actuator.register_observer(queue, detection, events, owner).map_err(IrisError::Observer)?; }
        let timer = if timer_wait { self.timer_registration.take() } else { None };
        self.pending = Some(Pending { client, owner, request, control: None, command: None, observed: false, controls_required, timer, timer_armed: false });
        if controls_required { actuator.control(queue, Some(owner), true, Selector(selector)).map_err(IrisError::Actuator)?; }
        Ok(())
    }

    pub fn command<B: DetectionIo>(
        &mut self, completion: ActuatorCompletion, context: &mut MotionContext,
        actuator: &mut LensActuatorCpu, detection: &DetectionProvider<'_, B>, events: &ActuatorEvents,
        task99: &Task99Client<'_>,
    ) -> Result<Option<IrisCompletion>, IrisError<core::convert::Infallible>> {
        let Some(mut pending) = self.pending else { return Ok(None); };
        if pending.owner != completion.owner { return Ok(None); }
        if completion.operation == ActuatorOperation::Control && pending.controls_required {
            pending.control = Some(completion.failed);
        } else if completion.operation == pending.request.operation() {
            pending.command = Some(completion.failed);
            if !completion.failed {
                context.iris_activity = match pending.request {
                    Request::Move(request) if request.policy.0 != 0 => AxisMotion::Forward,
                    Request::Move(_) => AxisMotion::Reverse,
                    Request::Home => AxisMotion::Idle,
                };
            }
        } else { return Err(IrisError::UnexpectedReply); }
        if let Some(callback) = pending.timer.filter(|_| !pending.timer_armed && pending.command == Some(false) &&
            (!pending.controls_required || pending.control == Some(false))) {
            task99.configure_periodic(PeriodicIndex::new(2).unwrap(), TickCount(10), PeriodicModes::new(1).unwrap(), Some(CallbackToken(callback.key().get()))).map_err(IrisError::Task99)?;
            pending.timer_armed = true;
        }
        self.pending = Some(pending);
        self.finish(context, actuator, detection, events)
    }

    pub fn observed<B: DetectionIo>(
        &mut self, completion: ObserverCompletion, context: &mut MotionContext,
        actuator: &mut LensActuatorCpu, detection: &DetectionProvider<'_, B>, events: &ActuatorEvents,
    ) -> Result<Option<IrisCompletion>, IrisError<core::convert::Infallible>> {
        let Some(mut pending) = self.pending else { return Ok(None); };
        if pending.owner != completion.owner || pending.timer.is_some() { return Ok(None); }
        pending.observed = true;
        self.pending = Some(pending);
        self.finish(context, actuator, detection, events)
    }

    pub fn periodic_completed<B: DetectionIo>(
        &mut self, callback: RegisteredCallbackId, context: &mut MotionContext,
        actuator: &mut LensActuatorCpu, detection: &DetectionProvider<'_, B>, events: &ActuatorEvents,
        task99: &Task99Client<'_>,
    ) -> Result<Option<IrisCompletion>, IrisError<core::convert::Infallible>> {
        let Some(mut pending) = self.pending else { return Ok(None); };
        if pending.timer != Some(callback) || !pending.timer_armed { return Ok(None); }
        task99.cancel_periodic(PeriodicIndex::new(2).unwrap()).map_err(IrisError::Task99)?;
        pending.observed = true;
        self.pending = Some(pending);
        self.finish(context, actuator, detection, events)
    }

    fn finish<B: DetectionIo>(
        &mut self, context: &mut MotionContext, actuator: &mut LensActuatorCpu,
        detection: &DetectionProvider<'_, B>, events: &ActuatorEvents,
    ) -> Result<Option<IrisCompletion>, IrisError<core::convert::Infallible>> {
        let Some(pending) = self.pending.filter(|pending| pending.completed()) else { return Ok(None); };
        self.pending = None;
        context.iris_in_progress = false;
        if pending.timer.is_none() { actuator.unregister_observer(detection, events).map_err(IrisError::Detection)?; }
        let service_effect = match pending.request {
            Request::Move(request) if request.origin == IrisOrigin::Direct421d && request.source.0 == 10 &&
                (pending.command == Some(true) || pending.control == Some(true)) => context.service,
            _ => None,
        };
        Ok(Some(IrisCompletion { client: pending.client, publication: pending.request.publication(), service_effect }))
    }

    pub fn cancel<M: Mmio, P: Platform, I: InterruptController, B: DetectionIo>(
        &mut self, context: &mut MotionContext, actuator: &mut LensActuatorCpu,
        queue: &mut LensQueueRuntime<M, P, I>, detection: &DetectionProvider<'_, B>, events: &ActuatorEvents,
        task99: &Task99Client<'_>,
    ) -> Result<Option<IrisCompletion>, IrisError<WireError<P::Error, I::Error>>> {
        let Some(pending) = self.pending.take() else { return Ok(None); };
        context.iris_in_progress = false;
        let observer = if pending.timer.is_none() { actuator.unregister_observer(detection, events) } else { Ok(()) };
        let cancel = actuator.cancel(queue);
        let timer = if pending.timer_armed {
            task99.cancel_periodic(PeriodicIndex::new(2).unwrap())
        } else { Ok(()) };
        observer.map_err(IrisError::Detection)?;
        cancel.map_err(IrisError::Actuator)?;
        timer.map_err(IrisError::Task99)?;
        Ok(Some(IrisCompletion { client: pending.client, publication: pending.request.publication(), service_effect: None }))
    }
}

#[cfg(test)]
#[path = "af_iris_runtime_tests.rs"]
mod tests;
