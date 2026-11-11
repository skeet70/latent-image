use super::*;
use crate::audio_release::{AudioDevice, CodecStatus, CodecWord, SerialHandle};
use crate::audio_release::native::NativeAudioDevice;
use crate::audio_release::stream_construction::{Address, ConstructionIo, ConstructionMemory, SemaphoreHandle};
use crate::audio_release::stream_construction::native::{NativeGuard, NativeIo, NativeMemory};
use crate::primitives::ServiceStatus;
use crate::shutdown::{post_application_record, QueueId, RequestId, ServiceMessage, ShutdownRtos, TaskId, UiEvents, UiRouting};
use crate::shutdown_rtos::NativeShutdownRtos;
use core::ptr::{read_volatile, write_volatile};

unsafe extern "C" {
    fn xm1_power_resource_acquire(identifier: u32, handle: *mut u32) -> i32;
    fn xm1_power_resource_release(handle: u32) -> i32;
    fn xm1_power_lcd_operation_action(address: u32) -> i32;
    fn xm1_power_operation_action(address: u32) -> i32;
    fn xm1_power_audio_bank_complete();
    fn xm1_power_audio_bank_notify(context: u32);
    fn xm1_power_audio_bank_notify_unchanged(context: u32);
}

struct NativePhysicalIo { record: *mut [u8; 76] }
impl AudioDevice for NativePhysicalIo {
    fn shadow(&self, register: crate::audio_release::CodecRegister) -> CodecWord { NativeAudioDevice.shadow(register) }
    fn store_shadow(&mut self, register: crate::audio_release::CodecRegister, value: CodecWord) { NativeAudioDevice.store_shadow(register, value); }
    fn open(&mut self) -> Result<SerialHandle, ServiceStatus> { NativeAudioDevice.open() }
    fn configure(&mut self, handle: SerialHandle) -> ServiceStatus { NativeAudioDevice.configure(handle) }
    fn write(&mut self, handle: SerialHandle, value: CodecWord) -> ServiceStatus { NativeAudioDevice.write(handle, value) }
    fn status(&mut self, handle: SerialHandle, status: &mut CodecStatus) -> ServiceStatus { NativeAudioDevice.status(handle, status) }
    fn stop(&mut self, handle: SerialHandle) -> ServiceStatus { NativeAudioDevice.stop(handle) }
    fn close(&mut self, handle: SerialHandle) -> ServiceStatus { NativeAudioDevice.close(handle) }
    fn lock_codec(&mut self) -> ServiceStatus { NativeAudioDevice.lock_codec() }
    fn unlock_codec(&mut self) -> ServiceStatus { NativeAudioDevice.unlock_codec() }
}
impl ConstructionIo for NativePhysicalIo {
    type Guard = NativeGuard;
    fn enter(&mut self) -> NativeGuard { NativeIo.enter() }
    fn leave(&mut self, guard: NativeGuard) { NativeIo.leave(guard); }
    fn wait(&mut self, semaphore: SemaphoreHandle) -> ServiceStatus { NativeIo.wait(semaphore) }
}
impl PhysicalIo for NativePhysicalIo {
    fn post(&mut self, _memory: &mut impl ConstructionMemory, command: RequestId) {
        let mut rtos = NativeShutdownRtos;
        let sender = rtos.current_task();
        let _ = rtos.post(ServiceMessage { operation: command, sender, destination: QueueId(41), payload: &[0; 4] });
    }
    fn panel(&mut self, _memory: &mut impl ConstructionMemory, operation: PanelOperation) -> Result<(), CallbackAddress> {
        let address = match operation { PanelOperation::Hold => 0x6d4960, PanelOperation::Release => 0x6d4978 };
        if unsafe { xm1_power_lcd_operation_action(address) } == 0 { Ok(()) } else { Err(CallbackAddress(address)) }
    }
    fn acquire_resource(&mut self, _memory: &mut impl ConstructionMemory, port: ClockPort, output: Address) -> ServiceStatus {
        let identifier = match port { ClockPort::Primary => 31, ClockPort::Secondary => 35 };
        ServiceStatus(unsafe { xm1_power_resource_acquire(identifier, output.0 as *mut u32) })
    }
    fn release_resource(&mut self, _memory: &mut impl ConstructionMemory, handle: ResourceHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_resource_release(handle.0) }) }
    fn callback(&mut self, _memory: &mut impl ConstructionMemory, callback: CallbackAddress) -> Result<(), CallbackAddress> {
        if unsafe { xm1_power_operation_action(callback.0) } == 0 { Ok(()) } else { Err(callback) }
    }
    fn signal(&mut self, _memory: &mut impl ConstructionMemory, task: TaskId, bits: u32) { let _ = NativeShutdownRtos.set_event_flag(u32::from(task.0), bits); }
    fn publish_bank(&mut self, _memory: &mut impl ConstructionMemory, publication: BankPublication) {
        let context = match publication {
            BankPublication::Explicit(context) => context,
            BankPublication::Unchanged => unsafe { xm1_power_audio_bank_notify_unchanged(0); return; },
        };
        if self.record.is_null() { unsafe { xm1_power_audio_bank_notify(context.0); } return; }
        unsafe {
            write_volatile(self.record as *mut u32, 0x816);
            write_volatile((self.record as *mut u32).add(1), context.0);
            let routing = if read_volatile(0x9134f8 as *const u32) == 1 { UiRouting::Primary } else { UiRouting::Other };
            let mut events = UiEvents::from_runtime(routing, TaskId(0));
            let _ = post_application_record(&mut NativeShutdownRtos, &mut events, &*self.record);
        }
    }
}

pub unsafe fn invoke(action: AudioAction) -> Result<(), u32> {
    unsafe {
        match action {
            AudioAction::BankComplete => { xm1_power_audio_bank_complete(); Ok(()) },
            AudioAction::BankNotify => { xm1_power_audio_bank_notify(0); Ok(()) },
            AudioAction::BankNotifyUnchanged => { xm1_power_audio_bank_notify_unchanged(0); Ok(()) },
            _ => super::invoke(&mut NativeMemory, &mut NativePhysicalIo { record: core::ptr::null_mut() }, action),
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_power_audio_bank_complete_backed(record: *mut [u8; 76]) -> i32 {
    let _ = super::invoke(&mut NativeMemory, &mut NativePhysicalIo { record }, AudioAction::BankComplete);
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_audio_bank_notify_backed(record: *mut [u8; 76], context: u32) -> i32 {
    NativePhysicalIo { record }.publish_bank(&mut NativeMemory, BankPublication::Explicit(BankNotificationContext(context)));
    0
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_audio_panel_control(value: u32) -> i32 {
    super::panel_control(&mut NativeMemory, &mut NativePhysicalIo { record: core::ptr::null_mut() }, value);
    0
}
