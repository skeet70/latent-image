use crate::area2::{Area2Lifecycle, FirmwareVersion, SaveObservation, SaveSelector};
use crate::mirror::{HeaderBackupDriver, SharedBacking};
use crate::record_store::{BlockBuffer, Callbacks, Entry, RecordMedium, Service, Staging};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransitionToken(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TaskId(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueId(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelStatus(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestId(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowerOffInput(());
impl PowerOffInput {
    pub const fn from_key_value(event: u32, value: u32) -> Option<Self> {
        if event == 6 && value == 0x80 { Some(Self(())) } else { None }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OuterState(u16);
impl OuterState {
    pub const fn from_abi(value: u16) -> Self { Self(value) }
    pub const fn abi(self) -> u16 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiRouting { Primary, Other }

pub struct UiEvents {
    routing: UiRouting,
    first_request_task: TaskId,
}
impl UiEvents {
    pub const fn new(routing: UiRouting) -> Self { Self { routing, first_request_task: TaskId(0) } }
    pub const fn from_runtime(routing: UiRouting, first_request_task: TaskId) -> Self { Self { routing, first_request_task } }
    pub const fn first_request_task(&self) -> Option<TaskId> {
        if self.first_request_task.0 == 0 { None } else { Some(self.first_request_task) }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionSource {
    Display,
    Calendar,
    Task7,
    Device,
    Task8,
    Auxiliary,
    Settings,
    Optional,
    Storage,
}
impl CompletionSource {
    pub const fn from_event(event: RequestId) -> Option<Self> {
        match event.0 {
            0x1200 => Some(Self::Display),
            0xac01 => Some(Self::Calendar),
            0x9701 => Some(Self::Task7),
            0x9a0c => Some(Self::Device),
            0x9801 => Some(Self::Task8),
            0xc901 => Some(Self::Auxiliary),
            0xe301 => Some(Self::Settings),
            0xf107 => Some(Self::Optional),
            0xf401 => Some(Self::Storage),
            _ => None,
        }
    }
    pub const fn bit(self) -> u16 {
        match self {
            Self::Display => 1,
            Self::Calendar => 2,
            Self::Task7 => 4,
            Self::Device => 8,
            Self::Task8 => 0x10,
            Self::Auxiliary => 0x20,
            Self::Settings => 0x40,
            Self::Optional => 0x80,
            Self::Storage => 0x100,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionDisposition { IgnoredToken, IgnoredDuplicate, Inactive, Accepted, Full, Late }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionObservation {
    pub disposition: CompletionDisposition,
    pub requests: crate::shutdown_completion::FollowOnObservation,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveArm { Close, Snapshot }
impl SaveArm {
    const fn selector(self) -> SaveSelector {
        match self { Self::Close => SaveSelector::Close, Self::Snapshot => SaveSelector::Snapshot }
    }
}

pub struct ServiceMessage<'a> {
    pub operation: RequestId,
    pub sender: TaskId,
    pub destination: QueueId,
    pub payload: &'a [u8],
}

// The service copies payload bytes before returning, as 0x72503c does.
pub trait ShutdownRtos {
    fn current_task(&mut self) -> TaskId;
    fn try_current_task(&mut self) -> Result<TaskId, KernelStatus> { Ok(self.current_task()) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus>;
    fn set_event_flag(&mut self, flag: u32, bits: u32) -> Result<(), KernelStatus>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleRequest { Save(TransitionToken), Terminal { argument: u32 } }

// 0xdb4b80 and 0xdb4bac preserve unwritten stack bytes in their payloads.
pub fn post_lifecycle(
    rtos: &mut impl ShutdownRtos,
    request: LifecycleRequest,
    caller_payload: &mut [u8; 16],
) -> Result<(), KernelStatus> {
    let sender = rtos.try_current_task()?;
    match request {
        LifecycleRequest::Save(token) => {
            caller_payload[..4].copy_from_slice(&token.0.to_le_bytes());
            rtos.post(ServiceMessage { operation: RequestId(2), sender, destination: QueueId(2), payload: &caller_payload[..4] })
        }
        LifecycleRequest::Terminal { argument } => {
            caller_payload[..4].copy_from_slice(&0x81u32.to_le_bytes());
            caller_payload[12..16].copy_from_slice(&argument.to_le_bytes());
            rtos.post(ServiceMessage { operation: RequestId(3), sender, destination: QueueId(2), payload: caller_payload })
        }
    }
}

pub fn post_transition(
    rtos: &mut impl ShutdownRtos,
    events: &mut UiEvents,
    request: RequestId,
    token: TransitionToken,
    caller_record: &mut [u8; 76],
) -> Result<(), KernelStatus> {
    caller_record[..4].copy_from_slice(&u32::from(request.0).to_le_bytes());
    caller_record[4..8].copy_from_slice(&token.0.to_le_bytes());
    post_application_record(rtos, events, caller_record)
}

pub fn post_application_record(
    rtos: &mut impl ShutdownRtos,
    events: &mut UiEvents,
    caller_record: &[u8; 76],
) -> Result<(), KernelStatus> {
    let request = u32::from_le_bytes(caller_record[..4].try_into().unwrap());
    let sender = rtos.try_current_task()?;
    let destination = match events.routing {
        UiRouting::Primary if request & 0x900 != 0 => QueueId(7),
        UiRouting::Primary | UiRouting::Other => QueueId(1),
    };
    let result = rtos.post(ServiceMessage { operation: RequestId(0), sender, destination, payload: caller_record });
    if request == 0x91b && events.first_request_task.0 == 0 {
        events.first_request_task = sender;
    }
    result
}


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownPhase { Idle, Saving, Waiting, Releasing, ReleaseFailed, CompletionPosted }

pub struct Shutdown {
    phase: ShutdownPhase,
    token: TransitionToken,
    completed: u16,
}
impl Shutdown {
    pub const fn new() -> Self {
        Self { phase: ShutdownPhase::Idle, token: TransitionToken(0), completed: 0 }
    }

    pub const fn phase(&self) -> ShutdownPhase { self.phase }

    pub fn save<D: HeaderBackupDriver, C: Callbacks, M: RecordMedium, S: Staging, B: BlockBuffer<M>>(
        &mut self,
        token: TransitionToken,
        arm: SaveArm,
        area2: &mut Area2Lifecycle<'_>,
        backing: &mut SharedBacking<'_, D>,
        version: FirmwareVersion,
        service: &mut Service<C>,
        entry: &mut Entry<'_, M, S, B>,
    ) -> Option<SaveObservation> {
        if self.phase != ShutdownPhase::Idle { return None; }
        self.token = token;
        self.phase = ShutdownPhase::Saving;
        Some(area2.save(arm.selector(), backing, version, service, entry))
    }


    pub fn receive_barrier_start(&mut self, operation: RequestId, token: TransitionToken) -> bool {
        if operation != RequestId(0x93e7) || self.phase != ShutdownPhase::Saving || token != self.token {
            return false;
        }
        self.completed = 0;
        self.phase = ShutdownPhase::Waiting;
        true
    }

    pub fn completion(
        &mut self,
        token: TransitionToken,
        source: CompletionSource,
        rtos: &mut impl ShutdownRtos,
        events: &mut UiEvents,
        notification_record: &mut [u8; 76],
        requests: &mut crate::shutdown_completion::CompletionRequests<'_>,
    ) -> Result<CompletionObservation, KernelStatus> {
        let without_requests = |disposition| CompletionObservation {
            disposition,
            requests: crate::shutdown_completion::FollowOnObservation::NONE,
        };
        if token != self.token { return Ok(without_requests(CompletionDisposition::IgnoredToken)); }
        if matches!(self.phase, ShutdownPhase::Releasing | ShutdownPhase::ReleaseFailed | ShutdownPhase::CompletionPosted) {
            post_transition(rtos, events, RequestId(0x91b), TransitionToken(0), notification_record)?;
            return Ok(without_requests(CompletionDisposition::Late));
        }
        if self.phase != ShutdownPhase::Waiting { return Ok(without_requests(CompletionDisposition::Inactive)); }
        let bit = source.bit();
        if self.completed & bit != 0 { return Ok(without_requests(CompletionDisposition::IgnoredDuplicate)); }
        self.completed |= bit;
        let requests = requests.submit(source, rtos);
        if self.completed == 0x1ff {
            self.phase = ShutdownPhase::Releasing;
            Ok(CompletionObservation { disposition: CompletionDisposition::Full, requests })
        } else {
            Ok(CompletionObservation { disposition: CompletionDisposition::Accepted, requests })
        }
    }


    pub fn reject_startup_cell(
        &mut self,
        rtos: &mut impl ShutdownRtos,
        events: &mut UiEvents,
        notification_record: &mut [u8; 76],
    ) -> Result<(), KernelStatus> {
        post_transition(rtos, events, RequestId(0x91b), TransitionToken(0), notification_record)
    }

    #[cfg(target_arch = "arm")]
    pub unsafe fn receive_barrier_start_native(
        &mut self,
        operation: RequestId,
        token: TransitionToken,
        storage_scratch: &mut [u8; 8],
    ) -> Option<Result<crate::shutdown_start::StartupObservation, KernelStatus>> {
        if !self.receive_barrier_start(operation, token) { return None; }
        unsafe { core::ptr::write_volatile(0x15c1e5c as *mut u32, 0) };
        Some(unsafe { crate::shutdown_start::start_native(storage_scratch) })
    }

    #[cfg(target_arch = "arm")]
    pub unsafe fn receive_completion_native(
        &mut self,
        operation: RequestId,
        task8_scratch: &mut [u8; 16],
        events: &mut UiEvents,
        notification_record: &mut [u8; 76],
    ) -> Result<Option<CompletionObservation>, KernelStatus> {
        let Some(source) = CompletionSource::from_event(operation) else { return Ok(None); };
        let mut requests = crate::shutdown_completion::CompletionRequests {
            task8_readiness: crate::shutdown_requests::Task8Readiness(unsafe {
                core::ptr::read_volatile(0x409c34c4 as *const u32)
            }),
            task8_completion: crate::shutdown_requests::OwnedCompletionAddress::task8_word(),
            calendar_completion: crate::shutdown_requests::OwnedCompletionAddress::calendar(),
            task8_payload: task8_scratch,
        };
        let observation = self.completion(
            self.token, source, &mut crate::shutdown_rtos::NativeShutdownRtos,
            events, notification_record, &mut requests,
        )?;
        unsafe { core::ptr::write_volatile(0x15c1e5c as *mut u32, u32::from(self.completed)) };
        if matches!(observation.disposition, CompletionDisposition::Full | CompletionDisposition::Late) {
            unsafe { core::ptr::write_volatile(0x15b8080 as *mut u16, 0) };
        }
        if observation.disposition == CompletionDisposition::Full {
            unsafe { self.finish_native(events, notification_record) }?;
        }
        Ok(Some(observation))
    }

    #[cfg(target_arch = "arm")]
    pub unsafe fn finish_native(
        &mut self,
        events: &mut UiEvents,
        notification_record: &mut [u8; 76],
    ) -> Result<(), KernelStatus> {
        extern "C" {
            fn xm1_power_charge_stop() -> i32;
            fn power_vg_release_output() -> i32;
            fn xm1_power_timers_stop() -> i32;
            fn xm1_power_audio_release(completion: u32) -> i32;
            fn xm1_power_audio_shutdown_callback() -> i32;
        }
        if self.phase != ShutdownPhase::Releasing { return Err(KernelStatus(-41)); }
        let _ = unsafe { xm1_power_charge_stop() };
        let status = unsafe { power_vg_release_output() };
        if status != 0 {
            self.phase = ShutdownPhase::ReleaseFailed;
            return Err(KernelStatus(status));
        }
        let _ = unsafe { xm1_power_timers_stop() };
        let _ = crate::shutdown_pins::transfer_pipeline_noop();
        let _ = unsafe { xm1_power_audio_release(xm1_power_audio_shutdown_callback as usize as u32) };
        // 0xec674c continues without testing 0xec6748's status.
        let mut observed = 0;
        let _ = unsafe { crate::shutdown_rtos::wait_current_task(1, &mut observed) };
        let mut pins = crate::shutdown_pins::PinLifecycle::from_abi(unsafe {
            core::ptr::read_volatile(0x9743e8 as *const u32)
        });
        pins.release(&mut crate::shutdown_pins::NativePins);
        unsafe { core::ptr::write_volatile(0x9743e8 as *mut u32, pins.abi()) };
        self.phase = ShutdownPhase::CompletionPosted;
        post_transition(
            &mut crate::shutdown_rtos::NativeShutdownRtos,
            events,
            RequestId(0x816),
            self.token,
            notification_record,
        )
    }
}
impl Default for Shutdown { fn default() -> Self { Self::new() } }

// 0xec6748 waits on the current task's flag, bit 1.
pub fn audio_release_completion(rtos: &mut impl ShutdownRtos) -> Result<(), KernelStatus> {
    rtos.set_event_flag(2, 1)
}

#[cfg(test)]
#[path = "shutdown_tests.rs"]
mod tests;
