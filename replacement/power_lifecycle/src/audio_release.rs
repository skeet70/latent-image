use crate::primitives::{destroy_queue, QueueRtos, QueueStorage, ServiceStatus};

#[path = "audio_executor.rs"]
pub mod executor;
#[path = "audio_callbacks.rs"]
pub mod callbacks;
#[path = "audio_capture_callbacks.rs"]
pub mod capture_callbacks;
#[path = "audio_stream.rs"]
pub mod stream;
#[path = "audio_packet_ring.rs"]
pub mod packet_ring;
#[path = "audio_stream_pipeline.rs"]
pub mod stream_pipeline;
#[path = "audio_stream_construction.rs"]
pub mod stream_construction;
#[path = "audio_stream_installed.rs"]
pub mod stream_installed;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CompletionAddress(u32);
impl CompletionAddress {
    pub const fn from_abi(value: u32) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }
    pub const fn address(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct AudioPhase(u32);
impl AudioPhase {
    pub const fn from_abi(value: u32) -> Self { Self(value) }
    pub const fn word(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseCommand { Request, SequenceFinished }
impl ReleaseCommand {
    pub const fn from_abi(value: u16) -> Option<Self> {
        match value { 0x3900 => Some(Self::Request), 0x3906 => Some(Self::SequenceFinished), _ => None }
    }
    pub const fn word(self) -> u16 { match self { Self::Request => 0x3900, Self::SequenceFinished => 0x3906 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReleaseMessage { pub command: ReleaseCommand, pub completion: Option<CompletionAddress> }
impl ReleaseMessage {
    pub fn decode(command: u16, payload: &[u8]) -> Result<Option<Self>, ServiceStatus> {
        let Some(command) = ReleaseCommand::from_abi(command) else { return Ok(None); };
        let Ok(payload) = <&[u8; 4]>::try_from(payload) else { return Err(ServiceStatus(-255)); };
        Ok(Some(Self { command, completion: CompletionAddress::from_abi(u32::from_le_bytes(*payload)) }))
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingAudioShutdown(());

pub trait AudioState {
    fn phase(&self) -> AudioPhase;
    fn set_phase(&mut self, phase: AudioPhase);
    fn completion(&self) -> Option<CompletionAddress>;
    fn set_completion(&mut self, completion: Option<CompletionAddress>);
}
pub trait AudioQueue {
    fn send(&mut self, message: ReleaseMessage) -> ServiceStatus;
}
pub trait AudioOperations {
    fn schedule_release(&mut self) -> ServiceStatus;
}
pub trait AudioCallbacks {
    fn invoke(&mut self, completion: CompletionAddress);
    fn retire_task41(&mut self) -> ServiceStatus;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodecRegister { Power, Output }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CodecWord(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct SerialHandle(pub u32);
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CodecStatus { pub receive: u32, pub transmit: u32, pub other: [u32; 5] }
pub trait AudioDevice {
    fn shadow(&self, register: CodecRegister) -> CodecWord;
    fn store_shadow(&mut self, register: CodecRegister, value: CodecWord);
    fn open(&mut self) -> Result<SerialHandle, ServiceStatus>;
    fn configure(&mut self, handle: SerialHandle) -> ServiceStatus;
    fn write(&mut self, handle: SerialHandle, value: CodecWord) -> ServiceStatus;
    fn status(&mut self, handle: SerialHandle, status: &mut CodecStatus) -> ServiceStatus;
    fn stop(&mut self, handle: SerialHandle) -> ServiceStatus;
    fn close(&mut self, handle: SerialHandle) -> ServiceStatus;
    fn lock_codec(&mut self) -> ServiceStatus;
    fn unlock_codec(&mut self) -> ServiceStatus;
}

pub fn request_release(state: &impl AudioState, queue: &mut impl AudioQueue, completion: Option<CompletionAddress>) -> Result<PendingAudioShutdown, ServiceStatus> {
    if state.phase().word() == 0 { return Err(ServiceStatus(-255)); }
    if queue.send(ReleaseMessage { command: ReleaseCommand::Request, completion }).0 == 0 {
        Ok(PendingAudioShutdown(()))
    } else { Err(ServiceStatus(-255)) }
}

pub fn write_codec(device: &mut impl AudioDevice, value: CodecWord) {
    let Ok(handle) = device.open() else { return; };
    if device.configure(handle).0 == 0 {
        if device.write(handle, value).0 == 0 {
            let mut status = CodecStatus::default();
            loop {
                let _ = device.status(handle, &mut status);
                if status.receive == 1 && status.transmit == 1 { break; }
            }
        }
        let _ = device.stop(handle);
    }
    let _ = device.close(handle);
}

pub fn run_release_action(device: &mut impl AudioDevice, queue: &mut impl AudioQueue, action: executor::Action) -> Result<(), executor::Action> {
    match action {
        executor::Action::None | executor::Action::Noop => (),
        executor::Action::OutputOff => {
            let value = CodecWord(device.shadow(CodecRegister::Output).0 & 0xfff3);
            device.store_shadow(CodecRegister::Output, value);
            write_codec(device, value);
        }
        executor::Action::PowerOff => {
            let _ = device.lock_codec();
            let value = CodecWord(device.shadow(CodecRegister::Power).0 & 0xffdf);
            device.store_shadow(CodecRegister::Power, value);
            write_codec(device, value);
            let _ = device.unlock_codec();
        }
        executor::Action::PostFinished => { let _ = queue.send(ReleaseMessage { command: ReleaseCommand::SequenceFinished, completion: None }); }
        other => return Err(other),
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseDispatch { NotSelected, SequenceScheduled(ServiceStatus), TaskRetired }

pub fn consume_release(
    state: &mut impl AudioState,
    operations: &mut impl AudioOperations,
    callbacks: &mut impl AudioCallbacks,
    rtos: &mut impl QueueRtos,
    storage: &mut impl QueueStorage,
    message: ReleaseMessage,
) -> ReleaseDispatch {
    match (message.command, state.phase().word()) {
        (ReleaseCommand::Request, 2 | 8) => {
            ReleaseDispatch::SequenceScheduled(schedule_received(state, operations, message.completion))
        }
        (ReleaseCommand::SequenceFinished, 6) => {
            finish_received(state, callbacks, rtos, storage)
        }
        _ => ReleaseDispatch::NotSelected,
    }
}

pub fn schedule_received(state: &mut impl AudioState, operations: &mut impl AudioOperations, completion: Option<CompletionAddress>) -> ServiceStatus {
    state.set_completion(completion);
    let status = operations.schedule_release();
    state.set_phase(AudioPhase(6));
    status
}

pub fn finish_received(state: &mut impl AudioState, callbacks: &mut impl AudioCallbacks, rtos: &mut impl QueueRtos, storage: &mut impl QueueStorage) -> ReleaseDispatch {
    let _ = destroy_queue(rtos, storage, 41);
    state.set_phase(AudioPhase(0));
    if let Some(completion) = state.completion() {
        callbacks.invoke(completion);
        state.set_completion(None);
    }
    let _ = callbacks.retire_task41();
    ReleaseDispatch::TaskRetired
}

pub fn signal_shutdown_complete(rtos: &mut impl crate::shutdown::ShutdownRtos) -> ServiceStatus {
    match rtos.set_event_flag(2, 1) { Ok(()) => ServiceStatus(0), Err(status) => ServiceStatus(status.0) }
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::*;
    use crate::callback_registry::{registry as callbacks, CallbackWord};
    use crate::callback_slots;
    use crate::primitives::{retire_current_task, CpuCore, NativeQueueStorage, OwnedQueueRtos, QueueDiagnostics};
    use crate::shutdown::{QueueId, RequestId, ServiceMessage, ShutdownRtos};
    use crate::shutdown_rtos::NativeShutdownRtos;

    extern "C" {
        fn xm1_rtos_semaphore_timed_wait(id: i32, timeout: i32) -> i32;
        fn xm1_rtos_semaphore_signal(id: i32) -> i32;
        fn xm1_power_serial_open(channel: u32, mode: u32, output: *mut u32) -> i32;
        fn xm1_power_serial_configure(handle: u32, configuration: u32) -> i32;
        fn xm1_power_serial_write_with_residue(handle: u32, value: u32, residue: u32) -> i32;
        fn xm1_power_serial_status(handle: u32, output: *mut u32) -> i32;
        fn xm1_power_serial_stop(handle: u32) -> i32;
        fn xm1_power_serial_close(handle: u32) -> i32;
    }

    unsafe fn load(address: u32) -> u32 { core::ptr::read_volatile(address as *const u32) }
    fn semaphore_id() -> i32 { unsafe { (41 | load(0x93e6f4 + 40 * 4) << 16) as i32 } }
    pub struct NativeAudioState;
    impl AudioState for NativeAudioState {
        fn phase(&self) -> AudioPhase { AudioPhase(unsafe { load(0x8faef0) }) }
        fn set_phase(&mut self, phase: AudioPhase) { unsafe { core::ptr::write_volatile(0x8faef0 as *mut u32, phase.word()) }; }
        fn completion(&self) -> Option<CompletionAddress> { CompletionAddress::from_abi(unsafe { load(0x8faf04) }) }
        fn set_completion(&mut self, completion: Option<CompletionAddress>) {
            unsafe { core::ptr::write_volatile(0x8faf04 as *mut u32, completion.map_or(0, CompletionAddress::address)) };
        }
    }
    pub struct NativeAudioQueue;
    impl AudioQueue for NativeAudioQueue {
        fn send(&mut self, message: ReleaseMessage) -> ServiceStatus {
            let mut rtos = NativeShutdownRtos;
            let sender = rtos.current_task();
            let payload = message.completion.map_or(0, CompletionAddress::address).to_le_bytes();
            match rtos.post(ServiceMessage {
                operation: RequestId(message.command.word()), sender, destination: QueueId(41), payload: &payload,
            }) {
                Ok(()) => ServiceStatus(0), Err(status) => ServiceStatus(status.0),
            }
        }
    }
    pub struct NativeAudioDevice;
    impl AudioDevice for NativeAudioDevice {
        fn shadow(&self, register: CodecRegister) -> CodecWord {
            let address = match register { CodecRegister::Power => 0x82916c, CodecRegister::Output => 0x82916e };
            CodecWord(unsafe { core::ptr::read_volatile(address as *const u16) })
        }
        fn store_shadow(&mut self, register: CodecRegister, value: CodecWord) {
            let address = match register { CodecRegister::Power => 0x82916c, CodecRegister::Output => 0x82916e };
            unsafe { core::ptr::write_volatile(address as *mut u16, value.0) };
        }
        fn open(&mut self) -> Result<SerialHandle, ServiceStatus> {
            let mut handle = 0;
            let status = unsafe { xm1_power_serial_open(0, 0, &mut handle) };
            if status == 0 { Ok(SerialHandle(handle)) } else { Err(ServiceStatus(status)) }
        }
        fn configure(&mut self, handle: SerialHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_configure(handle.0, 1) }) }
        fn write(&mut self, handle: SerialHandle, value: CodecWord) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_write_with_residue(handle.0, u32::from(value.0), 0) }) }
        fn status(&mut self, handle: SerialHandle, status: &mut CodecStatus) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_status(handle.0, status as *mut CodecStatus as *mut u32) }) }
        fn stop(&mut self, handle: SerialHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_stop(handle.0) }) }
        fn close(&mut self, handle: SerialHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_serial_close(handle.0) }) }
        fn lock_codec(&mut self) -> ServiceStatus { ServiceStatus(unsafe { xm1_rtos_semaphore_timed_wait(semaphore_id(), -1) }) }
        fn unlock_codec(&mut self) -> ServiceStatus { ServiceStatus(unsafe { xm1_rtos_semaphore_signal(semaphore_id()) }) }
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_audio_shutdown_callback() -> i32 {
        signal_shutdown_complete(&mut NativeShutdownRtos).0
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_audio_release(completion: u32) -> i32 {
        match request_release(&NativeAudioState, &mut NativeAudioQueue, CompletionAddress::from_abi(completion)) {
            Ok(_) => 0, Err(status) => status.0,
        }
    }

    pub struct NativeAudioCallbacks;
    impl AudioCallbacks for NativeAudioCallbacks {
        fn invoke(&mut self, completion: CompletionAddress) {
            let _ = callback_slots::call_no_arguments(callbacks(), CallbackWord(completion.address() as usize));
        }
        fn retire_task41(&mut self) -> ServiceStatus {
            unsafe { retire_current_task(crate::shutdown::TaskId(41)) }
        }
    }

    pub fn consume_received<D: QueueDiagnostics>(
        core: CpuCore, message: ReleaseMessage, diagnostics: D,
    ) -> ReleaseDispatch {
        consume_release(
            &mut NativeAudioState, &mut super::executor::native::NativeAudioOperations,
            &mut NativeAudioCallbacks, &mut OwnedQueueRtos,
            &mut NativeQueueStorage { core, diagnostics }, message,
        )
    }

    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_audio_schedule_release(payload: *const u32) -> i32 {
        let completion = CompletionAddress::from_abi(core::ptr::read_volatile(payload));
        let _ = schedule_received(&mut NativeAudioState, &mut super::executor::native::NativeAudioOperations, completion);
        0
    }

    #[no_mangle]
    pub extern "C" fn xm1_power_audio_destroy_task(_payload: *const u32) -> i32 {
        let _ = finish_received(
            &mut NativeAudioState, &mut NativeAudioCallbacks, &mut OwnedQueueRtos,
            &mut NativeQueueStorage { core: CpuCore::First, diagnostics: crate::diagnostics::NativeQueueDiagnostics },
        );
        0
    }
}

#[cfg(test)]
#[path = "audio_release_tests.rs"]
mod tests;
