//! X-M1 task50 focus motion CPU state and physical completion ownership.
//! Source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::af_capture_actor::{ClientId, LensMachineState, LensRequestConsumer, StateTransitionError};
use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_low::ConfigurationKey;
use crate::eeprom_detection::{DetectionIo, DetectionProvider, ServiceStatus};
use crate::lens_actuator_cpu::{self, ActuatorCompletion, ActuatorData, ActuatorError, ActuatorEvents, ActuatorOperation, ActuatorRequest, FocusOptions, LensActuatorCpu, LensPercentage, LensPosition, LensRate, MotionFlags, ObserverCompletion, ObserverError, OwnerToken, Selector};
use crate::lens_queue_runtime::LensQueueRuntime;
use xm1_lens_transport::control_runtime::Task99Client;
use xm1_lens_transport::wire::{Error as WireError, InterruptController, Mmio, Platform};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusDelta(pub i16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusPosition(pub i16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrisPosition(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ZoomPosition(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PositionSelection(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusRate(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusPercentage(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusRateShift(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AxisMotion { Idle, Forward, Reverse }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction { Forward, Reverse, Automatic }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AxisStatus { Idle, Active, Missing }
impl AxisStatus {
    pub const fn abi(self) -> u32 { match self { Self::Idle => 0, Self::Active => 1, Self::Missing => 2 } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceOperation { Request56, Request60, Request61, Request62, Request6b, Request70, Request71, Request7b, Unrelated }
impl ServiceOperation {
    pub const fn from_abi(value: u32) -> Self {
        match value {
            0x56 => Self::Request56, 0x60 => Self::Request60, 0x61 => Self::Request61,
            0x62 => Self::Request62, 0x6b => Self::Request6b, 0x70 => Self::Request70,
            0x71 => Self::Request71, 0x7b => Self::Request7b, _ => Self::Unrelated,
        }
    }
    pub fn report_failure(self, configuration: &mut ConfigImages<'_>) -> Result<(), BackendError> {
        let key = match self {
            Self::Request56 | Self::Request61 | Self::Request62 | Self::Request71 => 0x81,
            Self::Request60 => 0x86,
            Self::Request6b | Self::Request7b => 0x82,
            Self::Request70 => 0x83,
            Self::Unrelated => return Ok(()),
        };
        ConfigurationKey::from_abi(key).write(configuration, 1)
    }
}

pub struct MotionContext {
    pub current_focus: FocusPosition,
    pub current_iris: IrisPosition,
    pub current_zoom: ZoomPosition,
    pub zoom_reference: ZoomPosition,
    pub missing_focus: bool,
    pub missing_iris: bool,
    pub missing_zoom: bool,
    pub focus_in_progress: bool,
    pub iris_in_progress: bool,
    pub zoom_in_progress: bool,
    pub focus_activity: AxisMotion,
    pub iris_activity: AxisMotion,
    pub zoom_activity: AxisMotion,
    pub iris_irq_mode: bool,
    pub iris_fast_irq: bool,
    pub iris_source: crate::af_iris_runtime::IrisSource,
    pub motion_blocked: bool,
    pub direction: Direction,
    pub position_selection: PositionSelection,
    pub position_sample: ZoomPosition,
    pub previous_sample: ZoomPosition,
    pub zoom_profile_enabled: bool,
    pub service: Option<ServiceOperation>,
}
impl MotionContext {
    pub const fn initialized(initial_zoom: ZoomPosition) -> Self {
        Self { current_focus: FocusPosition(0), current_iris: IrisPosition(0), current_zoom: ZoomPosition(0), zoom_reference: initial_zoom,
            missing_focus: false, missing_iris: false, missing_zoom: false,
            focus_in_progress: false, iris_in_progress: false, zoom_in_progress: false,
            focus_activity: AxisMotion::Idle, iris_activity: AxisMotion::Idle, zoom_activity: AxisMotion::Idle,
            iris_irq_mode: false, iris_fast_irq: false, iris_source: crate::af_iris_runtime::IrisSource::from_abi(0), motion_blocked: false, direction: Direction::Automatic,
            position_selection: PositionSelection(-1), position_sample: ZoomPosition(0), previous_sample: ZoomPosition(0), zoom_profile_enabled: false, service: None }
    }
    pub const fn focus_status(&self) -> AxisStatus { axis_status(self.missing_focus, self.focus_in_progress) }
    pub const fn iris_status(&self) -> AxisStatus { axis_status(self.missing_iris, self.iris_in_progress) }
    pub const fn zoom_status(&self) -> AxisStatus { axis_status(self.missing_zoom, self.zoom_in_progress) }
    pub const fn effective_zoom(&self) -> ZoomPosition { if self.zoom_profile_enabled { self.current_zoom } else { ZoomPosition(0) } }
    pub fn take_position_sample(&mut self) -> ZoomPosition {
        let sample = self.previous_sample;
        self.position_sample = self.current_zoom;
        self.previous_sample = ZoomPosition(0);
        sample
    }
    pub const fn focus_target(&self, delta: FocusDelta) -> FocusPosition { FocusPosition(self.current_focus.0.saturating_add(delta.0)) }
    pub const fn application_zoom(&self) -> i32 { if self.missing_zoom { 0x80 } else { self.current_zoom.0 } }
    pub fn application_focus(&self, configuration: &mut ConfigImages<'_>) -> Result<FocusPosition, BackendError> {
        if self.missing_focus { ConfigurationKey::from_abi(0x81).write(configuration, 1)?; Ok(FocusPosition(0)) } else { Ok(self.current_focus) }
    }
    pub fn report_focus_completion(&self, home: bool, configuration: &mut ConfigImages<'_>) -> Result<(), BackendError> {
        let expected = if home { ServiceOperation::Request61 } else { ServiceOperation::Request60 };
        if self.missing_focus && self.service == Some(expected) { expected.report_failure(configuration)?; }
        Ok(())
    }
}
const fn axis_status(missing: bool, active: bool) -> AxisStatus {
    if missing { AxisStatus::Missing } else if active { AxisStatus::Active } else { AxisStatus::Idle }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusRequest {
    Home,
    Query,
    Rate { target: FocusPosition, rate: FocusRate },
    ScaledRate { target: FocusPosition, rate: FocusRate, shift: FocusRateShift },
    Percentage { target: FocusPosition, percentage: FocusPercentage },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionCompletion { pub client: ClientId }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MotionStart { Waiting, Duplicate, Completed(MotionCompletion) }
#[derive(Debug, Eq, PartialEq)]
pub enum MotionError<E> {
    Actuator(ActuatorError<E>), Observer(ObserverError<E>), Detection(ServiceStatus),
    State(StateTransitionError), Uninitialized(LensMachineState), Reply,
}
#[derive(Clone, Copy)]
struct Pending {
    client: ClientId, owner: OwnerToken, request: FocusRequest,
    control_failed: Option<bool>, command_failed: Option<bool>, observer_seen: bool,
}
pub struct FocusMotionRuntime { pending: Option<Pending> }
impl Default for FocusMotionRuntime { fn default() -> Self { Self::new() } }
impl FocusMotionRuntime {
    pub const fn new() -> Self { Self { pending: None } }

    pub fn begin<M: Mmio, P: Platform, I: InterruptController, B: DetectionIo>(
        &mut self, request: FocusRequest, client: ClientId, context: &mut MotionContext,
        machine: &mut LensRequestConsumer, actuator: &mut LensActuatorCpu,
        queue: &mut LensQueueRuntime<M, P, I>, task99: &Task99Client<'_>,
        detection: &DetectionProvider<'_, B>, events: &ActuatorEvents,
    ) -> Result<MotionStart, MotionError<WireError<P::Error, I::Error>>> {
        if self.pending.is_some() { return Ok(MotionStart::Duplicate); }
        let state = machine.machine_state();
        if request != FocusRequest::Query {
            match state.snapshot() {
                0 | 1 => return Err(MotionError::Uninitialized(state)),
                2 => (),
                _ => return Ok(MotionStart::Completed(MotionCompletion { client })),
            }
            if context.motion_blocked && request != FocusRequest::Home { return Ok(MotionStart::Completed(MotionCompletion { client })); }
        }
        if lens_actuator_cpu::not_suspended(task99) || (context.focus_in_progress && request != FocusRequest::Query) {
            return Ok(MotionStart::Completed(MotionCompletion { client }));
        }
        let owner = OwnerToken(queue.allocate_callback().map_err(|error| MotionError::Actuator(ActuatorError::Queue(error)))?.0);
        let pending = Pending { client, owner, request, control_failed: None, command_failed: None, observer_seen: false };
        self.pending = Some(pending);
        if request == FocusRequest::Query {
            actuator.execute(queue, Some(owner), ActuatorRequest::QuerySigned { selector: Selector(0), first: false, second: true }).map_err(MotionError::Actuator)?;
            return Ok(MotionStart::Waiting);
        }
        machine.transition_when_idle(state, LensMachineState::from_snapshot(15).unwrap()).map_err(MotionError::State)?;
        actuator.set_detection_mode(detection, false).map_err(MotionError::Detection)?;
        actuator.register_observer(queue, detection, events, owner).map_err(MotionError::Observer)?;
        actuator.control(queue, Some(owner), true, Selector(2)).map_err(MotionError::Actuator)?;
        context.focus_in_progress = true;
        match request {
            FocusRequest::Home => actuator.home(queue, Some(owner)).map_err(MotionError::Actuator)?,
            FocusRequest::Rate { target, rate } => actuator.position_rate(queue, Some(owner), LensPosition(target.0 as u16), LensRate(rate.0.min(0x1fff) as u16), MotionFlags { first: false, second: false, third: true }).map_err(MotionError::Actuator)?,
            FocusRequest::Percentage { target, percentage } => actuator.position_percentage(queue, Some(owner), LensPercentage(percentage.0.min(100) as u8), LensPosition(target.0 as u16)).map_err(MotionError::Actuator)?,
            FocusRequest::ScaledRate { target, rate, shift } => {
                let shifted = rate.0.checked_shr(u32::from(shift.0)).unwrap_or(0);
                let drive = if (shifted as i16) < 0 { 0x7fff } else { shifted as u16 };
                actuator.focus(queue, Some(owner), LensRate(drive), LensPosition(target.0 as u16), FocusOptions { first: false, second: false, third: true, mode: Selector(0), speed: Selector(0) }, task99.firmware_version()).map_err(MotionError::Actuator)?;
            }
            FocusRequest::Query => unreachable!(),
        }
        Ok(MotionStart::Waiting)
    }

    pub fn command<B: DetectionIo>(&mut self, completion: ActuatorCompletion, context: &mut MotionContext, machine: &mut LensRequestConsumer, actuator: &mut LensActuatorCpu, detection: &DetectionProvider<'_, B>, events: &ActuatorEvents) -> Result<Option<MotionCompletion>, MotionError<core::convert::Infallible>> {
        let Some(mut pending) = self.pending else { return Ok(None); };
        if pending.owner != completion.owner { return Ok(None); }
        if pending.request == FocusRequest::Query {
            if completion.operation != ActuatorOperation::QuerySigned { return Err(MotionError::Reply); }
            if !completion.failed {
                let ActuatorData::SignedPosition(value) = completion.data else { return Err(MotionError::Reply); };
                if value != i16::MIN { context.current_focus = FocusPosition(value); }
            }
            self.pending = None;
            return Ok(Some(MotionCompletion { client: pending.client }));
        }
        let expected = match pending.request {
            FocusRequest::Home => ActuatorOperation::Home,
            FocusRequest::Rate { .. } => ActuatorOperation::PositionRate,
            FocusRequest::Percentage { .. } => ActuatorOperation::PositionPercentage,
            FocusRequest::ScaledRate { .. } => ActuatorOperation::Focus,
            FocusRequest::Query => unreachable!(),
        };
        if completion.operation == ActuatorOperation::Control {
            if pending.control_failed.is_some() { return Ok(None); }
            pending.control_failed = Some(completion.failed);
        } else if completion.operation == expected {
            if pending.command_failed.is_some() { return Ok(None); }
            pending.command_failed = Some(completion.failed);
        } else { return Err(MotionError::Reply); }
        self.pending = Some(pending);
        self.finish_if_ready(context, machine, actuator, detection, events)
    }

    pub fn observer<B: DetectionIo>(&mut self, completion: ObserverCompletion, context: &mut MotionContext, machine: &mut LensRequestConsumer, actuator: &mut LensActuatorCpu, detection: &DetectionProvider<'_, B>, events: &ActuatorEvents) -> Result<Option<MotionCompletion>, MotionError<core::convert::Infallible>> {
        let Some(pending) = self.pending else { return Ok(None); };
        if pending.owner != completion.owner || pending.request == FocusRequest::Query { return Ok(None); }
        self.pending = Some(Pending { observer_seen: true, ..pending });
        self.finish_if_ready(context, machine, actuator, detection, events)
    }

    fn finish_if_ready<B: DetectionIo>(&mut self, context: &mut MotionContext, machine: &mut LensRequestConsumer, actuator: &mut LensActuatorCpu, detection: &DetectionProvider<'_, B>, events: &ActuatorEvents) -> Result<Option<MotionCompletion>, MotionError<core::convert::Infallible>> {
        let Some(pending) = self.pending else { return Ok(None); };
        let (Some(control_failed), Some(command_failed)) = (pending.control_failed, pending.command_failed) else { return Ok(None); };
        if !control_failed && !command_failed && !pending.observer_seen {
            if pending.request != FocusRequest::Home { context.focus_activity = AxisMotion::Reverse; }
            return Ok(None);
        }
        actuator.unregister_observer(detection, events).map_err(MotionError::Detection)?;
        machine.transition_when_idle(LensMachineState::from_snapshot(15).unwrap(), LensMachineState::from_snapshot(2).unwrap()).map_err(MotionError::State)?;
        self.pending = None;
        Ok(Some(MotionCompletion { client: pending.client }))
    }
}

#[cfg(test)]
#[path = "af_motion_runtime_tests.rs"]
mod tests;
