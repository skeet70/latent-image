use crate::primitives::ServiceStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct NodeAddress(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct OperationHandle(pub u32);
pub trait OperationOutput { fn store(&mut self, handle: OperationHandle); }
impl OperationOutput for OperationHandle {
    fn store(&mut self, handle: OperationHandle) { *self = handle; }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct DelayTicks(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutorMode { Disabled, Idle, Running }
impl ExecutorMode {
    pub const fn from_abi(value: u32) -> Self { match value { 1 => Self::Idle, 2 => Self::Running, _ => Self::Disabled } }
    pub const fn word(self) -> u32 { match self { Self::Disabled => 0, Self::Idle => 1, Self::Running => 2 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotState { Free, Active, Reserved }
impl SlotState {
    pub const fn from_abi(value: u32) -> Self { match value { 1 => Self::Active, 2 => Self::Reserved, _ => Self::Free } }
    pub const fn word(self) -> u32 { match self { Self::Free => 0, Self::Active => 1, Self::Reserved => 2 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationSlot { pub state: SlotState, pub remaining: DelayTicks, pub node: NodeAddress }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action { None, Noop, OutputOff, PowerOff, PostFinished, Audio(super::callbacks::AudioAction), Capture(super::capture_callbacks::CaptureAction), Stream(super::stream::Continuation), Pipeline(super::stream_pipeline::Completion), Other(u32) }
impl Action {
    pub const fn from_abi(value: u32) -> Self {
        match value {
            0 => Self::None, 0x6e6de4 => Self::Noop, 0x6e87c8 => Self::OutputOff, 0x6e878c => Self::PowerOff, 0x6e706c => Self::PostFinished,
            other => match super::callbacks::AudioAction::from_abi(other) {
                Some(action) => Self::Audio(action),
                None => match super::capture_callbacks::CaptureAction::from_abi(other) {
                    Some(action) => Self::Capture(action),
                    None => match super::stream::Continuation::from_abi(other) {
                        Some(action) => Self::Stream(action),
                        None => match super::stream_pipeline::Completion::from_abi(other) { Some(action) => Self::Pipeline(action), None => Self::Other(other) },
                    },
                },
            },
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationNode { pub delay: DelayTicks, pub action: Action, pub next: Option<NodeAddress> }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutorDiagnostic { Unavailable, Capacity, Generation }
pub trait ExecutorMemory {
    fn mode(&self) -> ExecutorMode;
    fn set_mode(&mut self, mode: ExecutorMode);
    fn slot(&self, index: usize) -> OperationSlot;
    fn set_slot(&mut self, index: usize, slot: OperationSlot);
    fn state(&self, index: usize) -> SlotState { self.slot(index).state }
    fn remaining(&self, index: usize) -> DelayTicks { self.slot(index).remaining }
    fn current(&self, index: usize) -> NodeAddress { self.slot(index).node }
    fn set_state(&mut self, index: usize, state: SlotState) { let mut slot = self.slot(index); slot.state = state; self.set_slot(index, slot); }
    fn set_remaining(&mut self, index: usize, remaining: DelayTicks) { let mut slot = self.slot(index); slot.remaining = remaining; self.set_slot(index, slot); }
    fn set_current(&mut self, index: usize, node: NodeAddress) { let mut slot = self.slot(index); slot.node = node; self.set_slot(index, slot); }
    fn generation(&self, index: usize) -> OperationHandle;
    fn set_generation(&mut self, index: usize, generation: OperationHandle);
    fn generation_counter(&self) -> OperationHandle;
    fn set_generation_counter(&mut self, value: OperationHandle);
    fn node(&self, address: NodeAddress) -> OperationNode;
    fn node_delay(&self, address: NodeAddress) -> DelayTicks { self.node(address).delay }
    fn node_action(&self, address: NodeAddress) -> Action { self.node(address).action }
    fn node_next(&self, address: NodeAddress) -> Option<NodeAddress> { self.node(address).next }
}
pub trait ExecutorIo {
    type SavedIrq;
    fn save_irq(&mut self) -> Self::SavedIrq;
    fn restore_irq(&mut self, saved: Self::SavedIrq);
    fn start_tick_timer(&mut self) -> ServiceStatus;
    fn stop_tick_timer(&mut self) -> ServiceStatus;
    fn invoke(&mut self, action: Action) -> Result<(), Action>;
    fn diagnostic(&mut self, diagnostic: ExecutorDiagnostic);
}

fn allocate_generation(memory: &mut impl ExecutorMemory, index: usize) -> Result<(), ServiceStatus> {
    if memory.generation(index).0 != 0 { return Err(ServiceStatus(-20)); }
    loop {
        memory.set_generation_counter(OperationHandle(memory.generation_counter().0.wrapping_add(1)));
        if memory.generation_counter().0 == 0 {
            memory.set_generation_counter(OperationHandle(memory.generation_counter().0.wrapping_add(1)));
        }
        if (0..10).all(|other| memory.generation(other) != memory.generation_counter()) {
            memory.set_generation(index, memory.generation_counter());
            return Ok(());
        }
    }
}

pub fn schedule(memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo, node: NodeAddress, handle: &mut impl OperationOutput) -> ServiceStatus {
    if memory.mode() == ExecutorMode::Disabled {
        io.diagnostic(ExecutorDiagnostic::Unavailable);
        return ServiceStatus(-255);
    }
    let saved = io.save_irq();
    let selected = (0..10).find(|&index| memory.state(index) == SlotState::Free);
    if let Some(index) = selected { memory.set_state(index, SlotState::Reserved); }
    io.restore_irq(saved);
    let Some(index) = selected else {
        io.diagnostic(ExecutorDiagnostic::Capacity);
        return ServiceStatus(-20);
    };
    let saved = io.save_irq();
    let generation = allocate_generation(memory, index);
    io.restore_irq(saved);
    if let Err(status) = generation {
        io.diagnostic(ExecutorDiagnostic::Generation);
        return status;
    }
    handle.store(memory.generation(index));
    memory.set_current(index, node);
    memory.set_remaining(index, memory.node_delay(node));
    if memory.remaining(index).0 == 0 { memory.set_remaining(index, DelayTicks(1)); }
    let saved = io.save_irq();
    memory.set_state(index, SlotState::Active);
    let status = if memory.mode() == ExecutorMode::Idle {
        let status = io.start_tick_timer();
        memory.set_mode(ExecutorMode::Running);
        status
    } else { ServiceStatus(0) };
    io.restore_irq(saved);
    status
}

fn find_generation(memory: &impl ExecutorMemory, handle: OperationHandle) -> Result<usize, ServiceStatus> {
    if handle.0 == 0 { return Err(ServiceStatus(-30)); }
    (0..10).find(|&index| memory.generation(index) == handle).ok_or(ServiceStatus(-30))
}

pub fn cancel(memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo, handle: OperationHandle) -> ServiceStatus {
    let saved = io.save_irq();
    let result = match find_generation(memory, handle) {
        Err(status) => status,
        Ok(index) => {
            memory.set_state(index, SlotState::Free);
            memory.set_generation(index, OperationHandle(0));
            if (0..10).all(|index| memory.state(index) == SlotState::Free) && memory.mode() == ExecutorMode::Running {
                let _ = io.stop_tick_timer();
                memory.set_mode(ExecutorMode::Idle);
            }
            ServiceStatus(0)
        }
    };
    io.restore_irq(saved);
    result
}

pub fn current_node(memory: &impl ExecutorMemory, io: &mut impl ExecutorIo, handle: OperationHandle) -> Result<NodeAddress, ServiceStatus> {
    match find_generation(memory, handle) {
        Ok(index) => Ok(memory.current(index)),
        Err(status) => { io.diagnostic(ExecutorDiagnostic::Generation); Err(status) }
    }
}

pub fn tick(memory: &mut impl ExecutorMemory, io: &mut impl ExecutorIo) -> Result<(), Action> {
    if memory.mode() != ExecutorMode::Running { return Ok(()); }
    for index in 0..10 {
        if memory.state(index) != SlotState::Active { continue; }
        memory.set_remaining(index, DelayTicks(memory.remaining(index).0.wrapping_sub(1)));
        while memory.remaining(index).0 == 0 {
            let action = memory.node_action(memory.current(index));
            if action != Action::None { io.invoke(memory.node_action(memory.current(index)))?; }
            let Some(next) = memory.node_next(memory.current(index)) else {
                memory.set_generation(index, OperationHandle(0));
                memory.set_state(index, SlotState::Free);
                break;
            };
            memory.set_current(index, next);
            memory.set_remaining(index, memory.node_delay(next));
        }
    }
    let saved = io.save_irq();
    if (0..10).all(|index| memory.state(index) != SlotState::Active) && memory.mode() != ExecutorMode::Idle {
        let _ = io.stop_tick_timer();
        memory.set_mode(ExecutorMode::Idle);
    }
    io.restore_irq(saved);
    Ok(())
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::*;
    use crate::audio_release::run_release_action;
    use crate::audio_release::native::{NativeAudioDevice, NativeAudioQueue};
    extern "C" {
        fn xm1_power_save_irq() -> u32;
        fn xm1_power_restore_irq(saved: u32);
        fn xm1_power_executor_timer_start_owned() -> i32;
        fn xm1_power_executor_timer_stop() -> i32;
        fn xm1_power_command_diagnostic(category: u32, code: u32) -> i32;
        fn xm1_power_lcd_operation_action(address: u32) -> i32;
    }
    unsafe fn load(address: u32) -> u32 { core::ptr::read_volatile(address as *const u32) }
    unsafe fn store(address: u32, value: u32) { core::ptr::write_volatile(address as *mut u32, value); }
    pub struct NativeExecutorMemory;
    impl ExecutorMemory for NativeExecutorMemory {
        fn mode(&self) -> ExecutorMode { ExecutorMode::from_abi(unsafe { load(0x9552f8) }) }
        fn set_mode(&mut self, mode: ExecutorMode) { unsafe { store(0x9552f8, mode.word()) }; }
        fn slot(&self, index: usize) -> OperationSlot {
            let address = 0x9552fc + index as u32 * 12;
            unsafe { OperationSlot { state: SlotState::from_abi(load(address)), remaining: DelayTicks(load(address + 4)), node: NodeAddress(load(address + 8)) } }
        }
        fn set_slot(&mut self, index: usize, slot: OperationSlot) {
            let address = 0x9552fc + index as u32 * 12;
            unsafe { store(address + 4, slot.remaining.0); store(address + 8, slot.node.0); store(address, slot.state.word()); }
        }
        fn state(&self, index: usize) -> SlotState { SlotState::from_abi(unsafe { load(0x9552fc + index as u32 * 12) }) }
        fn remaining(&self, index: usize) -> DelayTicks { DelayTicks(unsafe { load(0x955300 + index as u32 * 12) }) }
        fn current(&self, index: usize) -> NodeAddress { NodeAddress(unsafe { load(0x955304 + index as u32 * 12) }) }
        fn set_state(&mut self, index: usize, state: SlotState) { unsafe { store(0x9552fc + index as u32 * 12, state.word()) }; }
        fn set_remaining(&mut self, index: usize, remaining: DelayTicks) { unsafe { store(0x955300 + index as u32 * 12, remaining.0) }; }
        fn set_current(&mut self, index: usize, node: NodeAddress) { unsafe { store(0x955304 + index as u32 * 12, node.0) }; }
        fn generation(&self, index: usize) -> OperationHandle { OperationHandle(unsafe { load(0x955374 + index as u32 * 4) }) }
        fn set_generation(&mut self, index: usize, generation: OperationHandle) { unsafe { store(0x955374 + index as u32 * 4, generation.0) }; }
        fn generation_counter(&self) -> OperationHandle { OperationHandle(unsafe { load(0x409c2804) }) }
        fn set_generation_counter(&mut self, value: OperationHandle) { unsafe { store(0x409c2804, value.0) }; }
        fn node(&self, address: NodeAddress) -> OperationNode {
            unsafe { OperationNode { delay: DelayTicks(load(address.0)), action: Action::from_abi(load(address.0 + 4)), next: match load(address.0 + 8) { 0 => None, next => Some(NodeAddress(next)) } } }
        }
        fn node_delay(&self, address: NodeAddress) -> DelayTicks { DelayTicks(unsafe { load(address.0) }) }
        fn node_action(&self, address: NodeAddress) -> Action { Action::from_abi(unsafe { load(address.0 + 4) }) }
        fn node_next(&self, address: NodeAddress) -> Option<NodeAddress> { match unsafe { load(address.0 + 8) } { 0 => None, next => Some(NodeAddress(next)) } }
    }
    pub struct NativeExecutorIo<D> { pub diagnostics: D }
    pub trait OperationDiagnostics { fn report(&mut self, diagnostic: ExecutorDiagnostic); }
    impl<D: OperationDiagnostics> ExecutorIo for NativeExecutorIo<D> {
        type SavedIrq = u32;
        fn save_irq(&mut self) -> u32 { unsafe { xm1_power_save_irq() } }
        fn restore_irq(&mut self, saved: u32) { unsafe { xm1_power_restore_irq(saved) }; }
        fn start_tick_timer(&mut self) -> ServiceStatus {
            ServiceStatus(unsafe { xm1_power_executor_timer_start_owned() })
        }
        fn stop_tick_timer(&mut self) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_executor_timer_stop() }) }
        fn invoke(&mut self, action: Action) -> Result<(), Action> {
            match action {
                Action::Audio(action) => unsafe { super::super::callbacks::native::invoke(action).map_err(Action::Other) },
                Action::Capture(action) => {
                    use super::super::capture_callbacks::{invoke, native::{InstalledRouteFields, NativeCaptureHardware}};
                    invoke(&mut NativeCaptureHardware { routes: InstalledRouteFields }, action).map_err(Action::Other)
                }
                Action::Stream(continuation) => { super::super::stream::native::invoke(continuation); Ok(()) }
                Action::Pipeline(completion) => { unsafe { super::super::stream_pipeline::native::complete(completion); } Ok(()) }
                Action::Other(address) => {
                    if unsafe { xm1_power_lcd_operation_action(address) } == 0 { Ok(()) } else { Err(Action::Other(address)) }
                }
                owned => run_release_action(&mut NativeAudioDevice, &mut NativeAudioQueue, owned),
            }
        }
        fn diagnostic(&mut self, diagnostic: ExecutorDiagnostic) { self.diagnostics.report(diagnostic); }
    }
    pub struct NativeOperationDiagnostics;
    impl OperationDiagnostics for NativeOperationDiagnostics {
        fn report(&mut self, diagnostic: ExecutorDiagnostic) {
            let code = match diagnostic { ExecutorDiagnostic::Unavailable | ExecutorDiagnostic::Capacity => 43, ExecutorDiagnostic::Generation => 44 };
            unsafe { let _ = xm1_power_command_diagnostic(2, code); }
        }
    }
    struct SavedAudioHandle;
    impl OperationOutput for SavedAudioHandle {
        fn store(&mut self, handle: OperationHandle) { unsafe { store(0x8faef4, handle.0) }; }
    }
    pub struct NativeAudioOperations;
    impl crate::audio_release::AudioOperations for NativeAudioOperations {
        fn schedule_release(&mut self) -> ServiceStatus {
            schedule(&mut NativeExecutorMemory, &mut NativeExecutorIo { diagnostics: NativeOperationDiagnostics }, NodeAddress(0x7d1c28), &mut SavedAudioHandle)
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_audio_operation_tick() {
        if let Err(Action::Other(address)) = tick(&mut NativeExecutorMemory, &mut NativeExecutorIo { diagnostics: NativeOperationDiagnostics }) {
            core::arch::asm!("2: b 2b", in("r0") address, options(noreturn, nostack));
        }
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_operation_cancel(handle: u32) -> i32 {
        cancel(&mut NativeExecutorMemory, &mut NativeExecutorIo { diagnostics: NativeOperationDiagnostics }, OperationHandle(handle)).0
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_operation_current_node(handle: u32, output: *mut u32) -> i32 {
        match current_node(&NativeExecutorMemory, &mut NativeExecutorIo { diagnostics: NativeOperationDiagnostics }, OperationHandle(handle)) {
            Ok(node) => { core::ptr::write_volatile(output, node.0); 0 }
            Err(status) => status.0,
        }
    }

    struct AbiOperationOutput(*mut u32);
    impl OperationOutput for AbiOperationOutput {
        fn store(&mut self, handle: OperationHandle) { unsafe { core::ptr::write_volatile(self.0, handle.0) }; }
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_operation_submit(node: u32, handle: *mut u32) -> i32 {
        schedule(&mut NativeExecutorMemory, &mut NativeExecutorIo { diagnostics: NativeOperationDiagnostics }, NodeAddress(node), &mut AbiOperationOutput(handle)).0
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_operation_submit_interface(node: u32) -> i32 {
        let mut handle = 0;
        match xm1_power_operation_submit(node, &mut handle) { 0 => 0, -20 => 131, _ => 129 }
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_operation_action(address: u32) -> i32 {
        match (NativeExecutorIo { diagnostics: NativeOperationDiagnostics }).invoke(Action::from_abi(address)) {
            Ok(()) => 0,
            Err(_) => -255,
        }
    }
}

#[cfg(test)]
#[path = "audio_executor_tests.rs"]
mod tests;
