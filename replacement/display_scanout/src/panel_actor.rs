use crate::panel::{Completion, PanelCommand, PanelRequest, TaskId};
use crate::panel_device::{DeviceState, SequencePlan, SequenceStep};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitionTarget(u8);
impl TransitionTarget {
    pub const fn parse(value: u32) -> Option<Self> {
        if value < 14 { Some(Self(value as u8)) } else { None }
    }
    pub const fn code(self) -> u32 { self.0 as u32 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelMode { Normal, Alternate }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity { Idle, Busy, Held }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventFlags(u32);
impl EventFlags {
    pub const fn new(bits: u32) -> Self { Self(bits) }
    pub const fn bits(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event { Completed, Continue, SoftwareStep }
impl Event {
    pub const fn bits(self) -> u32 { match self { Self::Completed => 1, Self::Continue => 2, Self::SoftwareStep => 4 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message { Transition(PanelRequest), Timer, Unknown }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorError { Dispatch, InvalidRequest }
impl ActorError { pub const fn stock(self) -> i32 { match self { Self::Dispatch => -255, Self::InvalidRequest => -30 } } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationToken(pub u32);

pub trait ActorRuntime {
    fn build_plan(&mut self, current: TransitionTarget, target: TransitionTarget) -> SequencePlan;
    fn submit(&mut self, plan: &SequencePlan, start: usize, token: &mut OperationToken) -> i32;
    fn execute(&mut self, device: &mut DeviceState, step: SequenceStep) -> bool;
    fn invoke(&mut self, callback: Completion, state: &mut PanelState);
    fn clear_completion(&mut self);
    fn error(&mut self, callback: Completion);
    fn signal(&mut self, event: Event) -> i32;
    fn post_initial(&mut self) -> i32;
    fn receive(&mut self, message: &mut Message) -> i32;
    fn wait(&mut self, events: &mut EventFlags) -> i32;
    fn timer_command(&mut self, state: &mut PanelState);
}

pub struct PanelState {
    pub device_state: DeviceState,
    pub target: TransitionTarget,
    pub mode: PanelMode,
    pub enabled: bool,
    pub activity: Activity,
    pub events: EventFlags,
    pub message: Message,
    pub completion: Option<Completion>,
    pub error_callback: Option<Completion>,
    pub pending: bool,
    pub sequence: SequencePlan,
    pub operation_index: usize,
    pub operation_token: OperationToken,
}
impl PanelState {
    pub fn new(error_callback: Option<Completion>) -> Self {
        Self { device_state: DeviceState::new(), target: TransitionTarget(0), mode: PanelMode::Normal,
            enabled: true, activity: Activity::Idle, events: EventFlags(0), message: Message::Unknown,
            completion: None, error_callback, pending: true, sequence: SequencePlan::empty(),
            operation_index: 0, operation_token: OperationToken(0) }
    }
    pub fn initialize(&mut self, runtime: &mut impl ActorRuntime) {
        let _ = runtime.post_initial();
    }
    pub fn notify_error(&self, runtime: &mut impl ActorRuntime) {
        if let Some(callback) = self.error_callback { runtime.error(callback); }
    }
    pub fn signal(&self, event: Event, runtime: &mut impl ActorRuntime) {
        if runtime.signal(event) != 0 { self.notify_error(runtime); }
    }
    pub fn signal_complete(&mut self, target: TransitionTarget, runtime: &mut impl ActorRuntime) {
        self.target = target;
        self.operation_token = OperationToken(0);
        self.events = EventFlags(0);
        self.message = Message::Unknown;
        self.signal(Event::Completed, runtime);
    }
    pub fn complete(&mut self, runtime: &mut impl ActorRuntime) {
        if let Some(callback) = self.completion {
            runtime.invoke(callback, self);
            runtime.clear_completion();
            self.completion = None;
        }
        self.activity = Activity::Idle;
    }
    fn advance_linked(&mut self, start: usize) {
        let mut cursor = start;
        while let Some(step) = self.sequence.step(cursor) {
            if !step.linked { self.operation_index = cursor + 1; return; }
            cursor += 1;
        }
    }
    pub fn continue_sequence(&mut self, runtime: &mut impl ActorRuntime) {
        let start = self.operation_index;
        if start >= self.sequence.len() { self.complete(runtime); return; }
        self.advance_linked(start);
        let _ = runtime.submit(&self.sequence, start, &mut self.operation_token);
    }
    pub fn software_step(&mut self, runtime: &mut impl ActorRuntime) {
        let index = self.operation_index;
        if index >= self.sequence.len() { self.complete(runtime); return; }
        if let Some(step) = self.sequence.step(index).and_then(|step| step.callback) {
            if let SequenceStep::Complete(target) = step {
                if let Some(target) = TransitionTarget::parse(u32::from(target)) {
                    self.signal_complete(target, runtime);
                }
            } else if !runtime.execute(&mut self.device_state, step) {
                self.activity = Activity::Held;
                return;
            }
            self.operation_index += 1;
        }
        let start = self.operation_index;
        self.advance_linked(start);
        let _ = runtime.submit(&self.sequence, start, &mut self.operation_token);
    }
    pub fn begin(&mut self, request: PanelRequest, source: SourceClass, runtime: &mut impl ActorRuntime) {
        self.completion = request.completion;
        let target = select_target(self.mode, self.enabled, source, request.command);
        self.operation_index = 0;
        self.sequence = runtime.build_plan(self.target, target);
        if self.sequence.fault().is_some() {
            self.activity = Activity::Held;
            self.notify_error(runtime);
            return;
        }
        if self.sequence.len() == 0 {
            self.target = target;
            if let Some(callback) = self.completion { runtime.invoke(callback, self); }
        } else {
            self.advance_linked(0);
            let _ = runtime.submit(&self.sequence, 0, &mut self.operation_token);
            self.activity = Activity::Busy;
        }
    }
    pub fn dispatch(&mut self, source: SourceClass, runtime: &mut impl ActorRuntime) -> Result<(), ActorError> {
        match self.activity {
            Activity::Held => return Err(ActorError::Dispatch),
            Activity::Idle => match self.message {
                Message::Transition(request) => self.begin(request, source, runtime),
                Message::Timer => { if self.pending { runtime.timer_command(self); } },
                Message::Unknown => return Err(ActorError::Dispatch),
            },
            Activity::Busy => {
                if self.events.0 & 1 != 0 { self.complete(runtime); }
                else if self.events.0 & 2 != 0 { self.continue_sequence(runtime); }
                else if self.events.0 & 4 != 0 { self.software_step(runtime); }
                else { return Err(ActorError::Dispatch); }
            }
        }
        Ok(())
    }
    pub fn task_iteration(&mut self, source: SourceClass, runtime: &mut impl ActorRuntime) {
        let status = match self.activity {
            Activity::Idle => runtime.receive(&mut self.message),
            Activity::Busy | Activity::Held => runtime.wait(&mut self.events),
        };
        if status != 0 { self.notify_error(runtime); }
        if self.dispatch(source, runtime).is_err() { self.notify_error(runtime); }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceClass { Internal, External }
impl SourceClass {
    pub const fn parse(source: u32) -> Self { if source.wrapping_sub(2) <= 7 { Self::External } else { Self::Internal } }
}
pub fn select_target(mode: PanelMode, enabled: bool, source: SourceClass, command: PanelCommand) -> TransitionTarget {
    if command == PanelCommand::Command6 { return TransitionTarget(0); }
    if !matches!(command, PanelCommand::Command1 | PanelCommand::Command2 | PanelCommand::Command3) { return TransitionTarget(1); }
    let offset = u8::from(command == PanelCommand::Command2);
    let base = if enabled { if mode == PanelMode::Normal { 2 } else { 6 } }
        else if source == SourceClass::External { 10 }
        else if mode == PanelMode::Normal { 4 } else { 8 };
    TransitionTarget(base + offset)
}
pub fn decode_request(operation: u16, sender: TaskId, payload: &[u8]) -> Result<Message, ActorError> {
    match operation {
        0x3200 => {
            let bytes: &[u8; 8] = payload.try_into().map_err(|_| ActorError::InvalidRequest)?;
            let command = PanelCommand::parse(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).ok_or(ActorError::InvalidRequest)?;
            let completion = Completion::parse(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]));
            Ok(Message::Transition(PanelRequest { sender, command, completion }))
        }
        0x3201 => Ok(Message::Timer),
        _ => Ok(Message::Unknown),
    }
}

#[cfg(all(target_arch = "arm", feature = "panel"))]
mod abi {
    use super::*;
    use core::ptr::{addr_of_mut, read_volatile, write_volatile};
    use crate::panel_device::{native_execute, native_reset_pin, native_build_plan, native_read_callback, native_submit_existing, plan, ProgrammingOutcome};

    const RAM: usize = 0x0096bd58;
    static mut STATE: Option<PanelState> = None;
    struct Submission { start: usize }
    static mut SUBMISSION: Option<Submission> = None;
    static mut CALLBACK: Option<(Completion, bool)> = None;
    static mut ERROR: Option<Completion> = None;
    static mut SOFTWARE: Option<SequenceStep> = None;
    static mut TIMER: bool = false;
    static mut SIGNAL: Option<Event> = None;
    unsafe fn call(callback: Completion) {
        let callback: unsafe extern "C" fn() = core::mem::transmute(callback.address() as usize);
        callback();
    }
    unsafe fn flush_callbacks() {
        if let Some(callback) = (*addr_of_mut!(ERROR)).take() { call(callback); }
        if let Some((callback, clear_after)) = (*addr_of_mut!(CALLBACK)).take() {
            state().activity = if read(0x18) == 0 { Activity::Idle } else { Activity::Busy };
            state().completion = Completion::parse(read_volatile((read(8) + 4) as *const u32));
            call(callback);
            let payload = read(8);
            if clear_after {
                write_volatile((payload + 4) as *mut u32, 0);
                write(0x18, 0);
            }
            let actor = state();
            if let Some(target) = TransitionTarget::parse(read(0x1c)) { actor.target = target; }
            actor.events = EventFlags(read(0x14));
            actor.operation_index = read(0x1d0) as usize;
            actor.operation_token = OperationToken(read(0x48));
            actor.completion = Completion::parse(read_volatile((payload + 4) as *const u32));
            actor.activity = if read(0x18) == 0 { Activity::Idle } else { Activity::Busy };
        }
    }
    use crate::panel::ServiceDescriptor as Descriptor;
    extern "C" {
        fn xm1_power_queue_receive(queue: u32, descriptor: *mut Descriptor, timeout: i32) -> i32;
        fn xm1_power_wait_current_all(bits: u32, observed: *mut u32, timeout: i32) -> i32;
        fn xm1_power_set_task_flags(task: u32, bits: u32) -> i32;
        fn xm1_power_set_task_phase(task: u32, phase: u32) -> i32;
        fn xm1_power_operation_submit(node: u32, token: *mut u32) -> i32;
        fn xm1_power_current_task() -> u32;
        fn xm1_power_service_post(queue: u32, descriptor: *const Descriptor) -> i32;
    }
    unsafe fn read(offset: usize) -> u32 { read_volatile((RAM + offset) as *const u32) }
    unsafe fn write(offset: usize, value: u32) { write_volatile((RAM + offset) as *mut u32, value); }
    unsafe fn state() -> &'static mut PanelState {
        let slot = &mut *addr_of_mut!(STATE);
        if slot.is_none() { *slot = Some(PanelState::new(None)); }
        slot.as_mut().unwrap()
    }
    unsafe fn publish(state: &PanelState) {
        write(0x18, match state.activity { Activity::Idle => 0, Activity::Busy => 1, Activity::Held => 2 });
    }
    unsafe fn scan_live_boundaries(start: u32, count: u32) {
        let mut cursor = start;
        while cursor < count {
            let address = 0x96bdac_u32.wrapping_add(cursor.wrapping_mul(12));
            if read_volatile(address as *const u32) == 0 {
                let next = cursor.wrapping_add(1);
                write(0x1d0, next);
                state().operation_index = next as usize;
                return;
            }
            cursor = cursor.wrapping_add(1);
        }
    }
    unsafe fn finish_software_callback() {
        let start = read(0x1d0).wrapping_add(1);
        write(0x1d0, start);
        state().operation_index = start as usize;
        let count = read(0x1cc);
        scan_live_boundaries(start, count);
        crate::panel_device::read_stock_device_state(&mut state().device_state);
        *addr_of_mut!(SUBMISSION) = Some(Submission { start: start as usize });
    }
    unsafe fn flush_software() {
        let Some(step) = (*addr_of_mut!(SOFTWARE)).take() else { return; };
        state().operation_index = read(0x1d0) as usize;
        let mut device = DeviceState::new();
        crate::panel_device::read_stock_device_state(&mut device);
        let outcome = native_execute(&mut device, step);
        if outcome == ProgrammingOutcome::Held {
            *addr_of_mut!(SUBMISSION) = None;
            native_hold();
            return;
        }
        finish_software_callback();
    }
    unsafe fn flush_timer() {
        if !*addr_of_mut!(TIMER) { return; }
        *addr_of_mut!(TIMER) = false;
        let _ = xm1_power_set_task_phase(34, 7);
        if read(0x28) != 1 && crate::panel::native_reset_enabled() && read(0x1c) == 3 {
            let mut device = DeviceState::new();
            crate::panel_device::read_stock_device_state(&mut device);
            native_reset_pin(&mut device);
            crate::panel_device::read_stock_device_state(&mut state().device_state);
        }
        let _ = xm1_power_set_task_phase(34, 6);
        state().activity = if read(0x18) == 0 { Activity::Idle } else { Activity::Busy };
    }
    unsafe fn flush_signal() {
        let Some(event) = (*addr_of_mut!(SIGNAL)).take() else { return; };
        if event == Event::Completed {
            state().operation_index = read(0x1d0) as usize;
            write(0x1c, state().target.code());
            write(0x48, 0);
            write(0x14, 0);
            write_volatile(RAM as *mut u16, 0);
        }
        if xm1_power_set_task_flags(34, event.bits()) != 0 { xm1_scanout_panel_notify_error(); }
        if event == Event::Completed { finish_software_callback(); }
    }
    unsafe fn flush_submission() {
        let Some(submission) = (*addr_of_mut!(SUBMISSION)).take() else { return; };
        write(0x1d0, state().operation_index as u32);
        let node = native_submit_existing(submission.start);
        state().activity = if read(0x18) == 0 { Activity::Idle } else { Activity::Busy };
        let _ = xm1_power_operation_submit(node, (RAM + 0x48) as *mut u32);
        state().operation_token = OperationToken(read(0x48));
        state().operation_index = read(0x1d0) as usize;
        state().activity = if read(0x18) == 0 { Activity::Idle } else { Activity::Busy };
    }
    pub unsafe fn native_hold() { state().activity = Activity::Held; write(0x18, 2); }
    struct NativeRuntime;
    impl ActorRuntime for NativeRuntime {
        fn build_plan(&mut self, current: TransitionTarget, target: TransitionTarget) -> SequencePlan {
            plan(current.code(), target.code())
        }
        fn submit(&mut self, _: &SequencePlan, start: usize, _: &mut OperationToken) -> i32 {
            unsafe { *addr_of_mut!(SUBMISSION) = Some(Submission { start }); }
            0
        }
        fn execute(&mut self, _: &mut DeviceState, step: SequenceStep) -> bool {
            unsafe { *addr_of_mut!(SOFTWARE) = Some(step); }
            true
        }
        fn invoke(&mut self, callback: Completion, _: &mut PanelState) {
            unsafe { *addr_of_mut!(CALLBACK) = Some((callback, false)); }
        }
        fn clear_completion(&mut self) {
            unsafe {
                if let Some((_, clear_after)) = (&mut *addr_of_mut!(CALLBACK)).as_mut() { *clear_after = true; }
            }
        }
        fn error(&mut self, callback: Completion) { unsafe { *addr_of_mut!(ERROR) = Some(callback); } }
        fn signal(&mut self, event: Event) -> i32 {
            unsafe { *addr_of_mut!(SIGNAL) = Some(event); }
            0
        }
        fn post_initial(&mut self) -> i32 { unsafe { crate::panel::xm1_scanout_panel_command(0, 0) } }
        fn receive(&mut self, message: &mut Message) -> i32 {
            unsafe {
                let mut descriptor = Descriptor {
                    operation: read_volatile(RAM as *const u16),
                    sender: read_volatile((RAM + 2) as *const u8),
                    destination: read_volatile((RAM + 3) as *const u8),
                    length: read(4), payload: read(8),
                };
                let status = xm1_power_queue_receive(34, &mut descriptor, -1);
                if status != 0 { return status; }
                write_volatile(RAM as *mut u16, descriptor.operation);
                write_volatile((RAM + 2) as *mut u8, descriptor.sender);
                write_volatile((RAM + 3) as *mut u8, descriptor.destination);
                write(4, descriptor.length);
                write(8, descriptor.payload);
                let payload = if descriptor.length == 0 { &[][..] }
                    else { core::slice::from_raw_parts(descriptor.payload as *const u8, descriptor.length as usize) };
                match decode_request(descriptor.operation, TaskId(descriptor.sender), payload) {
                    Ok(decoded) => { *message = decoded; status }
                    Err(error) => { *message = Message::Unknown; error.stock() }
                }
            }
        }
        fn wait(&mut self, events: &mut EventFlags) -> i32 {
            unsafe {
                let status = xm1_power_wait_current_all(7, (RAM + 0x14) as *mut u32, -1);
                *events = EventFlags(read(0x14));
                status
            }
        }
        fn timer_command(&mut self, _: &mut PanelState) { unsafe { *addr_of_mut!(TIMER) = true; } }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_initialize(input: *const u32) {
        let error = Completion::parse(read_volatile(input));
        *addr_of_mut!(STATE) = Some(PanelState::new(error));
        write(4, 8);
        write(8, (RAM + 0xc) as u32);
        for (offset, value) in [
            (0x14, 0), (0x1c, 0), (0x48, 0),
            (0x20, 11), (0x24, 0), (0x28, 0), (0x2c, 1), (0x30, 0),
            (0x34, 0), (0x38, 1), (0x3c, 0), (0x40, 5), (0x44, 5),
            (0x1d4, error.map_or(0, Completion::address)), (0x1d8, 0),
            (0x1dc, 0), (0x1e0, 0), (0x1e8, 0), (0x1ec, 1), (0x24c, 0),
        ] { write(offset, value); }
        write_volatile((RAM + 0x1f0) as *mut u16, 0);
        publish(state());
        state().initialize(&mut NativeRuntime);
    }
    unsafe fn begin_native(request: PanelRequest) -> i32 {
        let mode = if read(0x28) == 0 { PanelMode::Normal } else { PanelMode::Alternate };
        let target = select_target(mode, read(0x2c) == 1, SourceClass::parse(read(0x24)), request.command);
        state().completion = request.completion;
        state().operation_index = 0;
        write(0x1d0, 0);
        let status = match native_build_plan(read(0x1c), target.code()) {
            Ok(status) => status,
            Err(status) => { native_hold(); return status; }
        };
        if read(0x1cc) == 0 {
            state().target = target;
            write(0x1c, target.code());
            if let Some(callback) = Completion::parse(read_volatile((read(8) + 4) as *const u32)) {
                let callback: unsafe extern "C" fn(i32) = core::mem::transmute(callback.address() as usize);
                callback(status);
            }
            state().activity = if read(0x18) == 0 { Activity::Idle } else { Activity::Busy };
            if let Some(target) = TransitionTarget::parse(read(0x1c)) { state().target = target; }
            state().events = EventFlags(read(0x14));
            state().operation_index = read(0x1d0) as usize;
            state().operation_token = OperationToken(read(0x48));
            return -11;
        }
        scan_live_boundaries(0, read(0x1cc));
        *addr_of_mut!(SUBMISSION) = Some(Submission { start: 0 });
        flush_submission();
        state().activity = Activity::Busy;
        write(0x18, 1);
        -12
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_dispatch() -> i32 {
        state().activity = match read(0x18) { 0 => Activity::Idle, 1 => Activity::Busy, _ => Activity::Held };
        state().events = EventFlags(read(0x14));
        if state().activity == Activity::Idle && read_volatile(RAM as *const u16) == 0x3200 {
            let payload = core::slice::from_raw_parts(read(8) as *const u8, read(4) as usize);
            let request = match decode_request(0x3200, TaskId(read_volatile((RAM + 2) as *const u8)), payload) {
                Ok(Message::Transition(request)) => request,
                _ => return ActorError::InvalidRequest.stock(),
            };
            return match begin_native(request) { -12 | -11 => 0, status => status };
        }
        let events = state().events.bits();
        if state().activity == Activity::Busy && events & 1 == 0 && events & 6 != 0 {
            if events & 2 != 0 { xm1_scanout_panel_continue(); }
            else { xm1_scanout_panel_software_step(); }
            return if state().activity == Activity::Held { -255 } else { 0 };
        }
        let result = {
            let actor = state();
            crate::panel_device::read_stock_device_state(&mut actor.device_state);
            actor.mode = if read(0x28) == 0 { PanelMode::Normal } else { PanelMode::Alternate };
            actor.enabled = read(0x2c) == 1;
            actor.pending = read(0x1ec) != 0;
            if actor.activity == Activity::Idle {
                let operation = read_volatile(RAM as *const u16);
                let length = read(4) as usize;
                let payload = if length == 0 { &[][..] } else { core::slice::from_raw_parts(read(8) as *const u8, length) };
                actor.message = decode_request(operation, TaskId(read_volatile((RAM + 2) as *const u8)), payload)
                    .unwrap_or(Message::Unknown);
            } else {
                actor.completion = Completion::parse(read_volatile((read(8) + 4) as *const u32));
            }
            actor.dispatch(SourceClass::parse(read(0x24)), &mut NativeRuntime)
        };
        flush_software();
        flush_timer();
        flush_signal();
        flush_submission();
        flush_callbacks();
        publish(state());
        if state().activity == Activity::Held { ActorError::Dispatch.stock() }
        else { result.map_or_else(ActorError::stock, |()| 0) }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_task(input: *const u32) -> ! {
        xm1_scanout_panel_initialize(input);
        loop {
            let activity = state().activity;
            let status = match activity {
                Activity::Idle => {
                    let mut message = state().message;
                    let status = NativeRuntime.receive(&mut message);
                    state().message = message;
                    status
                }
                Activity::Busy | Activity::Held => {
                    let mut events = state().events;
                    let status = NativeRuntime.wait(&mut events);
                    state().events = events;
                    status
                }
            };
            if status != 0 { xm1_scanout_panel_notify_error(); }
            if xm1_scanout_panel_dispatch() != 0 { xm1_scanout_panel_notify_error(); }
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_signal_complete(target: u32) -> i32 {
        let Some(target) = TransitionTarget::parse(target) else { return -30; };
        let actor = state();
        actor.target = target;
        actor.operation_token = OperationToken(0);
        actor.events = EventFlags(0);
        actor.message = Message::Unknown;
        write_volatile(RAM as *mut u16, 0);
        write(0x1c, target.code());
        write(0x48, 0);
        write(0x14, 0);
        if xm1_power_set_task_flags(34, 1) != 0 { xm1_scanout_panel_notify_error(); }
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_signal_event4() {
        if xm1_power_set_task_flags(34, 4) != 0 { xm1_scanout_panel_notify_error(); }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_signal_event2() {
        if xm1_power_set_task_flags(34, 2) != 0 { xm1_scanout_panel_notify_error(); }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_complete() -> i32 {
        state().completion = Completion::parse(read_volatile((read(8) + 4) as *const u32));
        state().complete(&mut NativeRuntime);
        flush_callbacks();
        publish(state());
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_continue() -> i32 {
        let cursor = read(0x1d0);
        state().operation_index = cursor as usize;
        let count = read(0x1cc);
        if cursor >= count {
            state().completion = Completion::parse(read_volatile((read(8) + 4) as *const u32));
            state().complete(&mut NativeRuntime);
        } else {
            scan_live_boundaries(cursor, count);
            *addr_of_mut!(SUBMISSION) = Some(Submission { start: cursor as usize });
        }
        flush_submission();
        flush_callbacks();
        publish(state());
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_software_step() -> i32 {
        let cursor = read(0x1d0);
        state().operation_index = cursor as usize;
        let count = read(0x1cc);
        if cursor >= count {
            state().completion = Completion::parse(read_volatile((read(8) + 4) as *const u32));
            state().complete(&mut NativeRuntime);
        } else {
            let current = match native_read_callback(cursor) {
                Ok(callback) => callback,
                Err(_) => { native_hold(); return -255; }
            };
            if let Some(step) = current {
                *addr_of_mut!(SOFTWARE) = Some(step);
                flush_software();
            } else {
                scan_live_boundaries(cursor, count);
                *addr_of_mut!(SUBMISSION) = Some(Submission { start: cursor as usize });
            }
        }
        flush_submission();
        flush_callbacks();
        publish(state());
        if state().activity == Activity::Held { -255 } else { 0 }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_notify_error() {
        if let Some(callback) = Completion::parse(read(0x1d4)) { call(callback); }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_timer_command() {
        *addr_of_mut!(TIMER) = true;
        flush_timer();
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_post_timer_command() {
        let descriptor = Descriptor {
            operation: 0x3201, sender: xm1_power_current_task() as u8,
            destination: 34, length: 0, payload: 0,
        };
        if xm1_power_service_post(34, &descriptor) != 0 { xm1_scanout_panel_notify_error(); }
    }
}
#[cfg(all(target_arch = "arm", feature = "panel"))]
pub use abi::*;
