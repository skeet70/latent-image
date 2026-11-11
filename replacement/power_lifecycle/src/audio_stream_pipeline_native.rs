use super::*;
use super::super::stream_construction::{Address, ConstructionMemory};
use super::super::stream_construction::native::{NativeGuard, NativeIo, NativeMemory};
use super::super::stream_installed::{CallbackAddress, InstalledIo, InstalledPipeline, IrqHandle, IrqSource};
use super::super::executor::NodeAddress;
extern "C" {
    fn xm1_power_save_irq() -> u32;
    fn xm1_power_restore_irq(saved: u32);
    fn xm1_power_timer_irq_attach(source: u32, handler: u32, output: *mut u32) -> i32;
    fn xm1_power_timer_release_handle(handle: u32) -> i32;
    fn xm1_power_dcache_clean_range(address: u32, length: u32);
    fn xm1_power_operation_action(address: u32) -> i32;
    fn xm1_power_operation_submit(node: u32, handle: *mut u32) -> i32;
    fn xm1_rtos_semaphore_signal(id: i32) -> i32;
}
pub struct SavedIrq(u32);
impl InstalledIo for NativeIo {
    type IrqMask = SavedIrq;
    fn save_irq(&mut self) -> SavedIrq { SavedIrq(unsafe { xm1_power_save_irq() }) }
    fn restore_irq(&mut self, saved: SavedIrq) { unsafe { xm1_power_restore_irq(saved.0); } }
    fn attach_irq(&mut self, _memory: &mut impl ConstructionMemory, source: IrqSource, completion: Completion, output: Address) -> ServiceStatus {
        let handler: unsafe extern "C" fn() = match completion { Completion::Receive => xm1_power_audio_stream_receive_complete, Completion::Send => xm1_power_audio_stream_send_complete };
        let handler = match register_completion(handler) { Ok(handler) => handler, Err(error) => return ServiceStatus(crate::callback_registry::refused(error)) };
        ServiceStatus(unsafe { xm1_power_timer_irq_attach(source.0, handler.0 as u32, output.0 as *mut u32) })
    }
    fn release_irq(&mut self, handle: IrqHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_power_timer_release_handle(handle.0) }) }
    fn clean_data_cache(&mut self) { unsafe { xm1_power_dcache_clean_range(0, 0); } }
    fn signal(&mut self, semaphore: super::super::stream_construction::SemaphoreHandle) -> ServiceStatus { ServiceStatus(unsafe { xm1_rtos_semaphore_signal(semaphore.0) }) }
    fn invoke(&mut self, _memory: &mut impl ConstructionMemory, callback: CallbackAddress) { unsafe {
        if xm1_power_operation_action(callback.0) != 0 { core::arch::asm!("2: b 2b", in("r0") callback.0, options(noreturn, nostack)); }
    } }
    fn schedule(&mut self, _memory: &mut impl ConstructionMemory, node: NodeAddress, output: Address) -> ServiceStatus {
        ServiceStatus(unsafe { xm1_power_operation_submit(node.0, output.0 as *mut u32) })
    }
}
fn installed<R>(residue: u32, operation: impl FnOnce(&mut InstalledPipeline<'_, NativeMemory, NativeIo>) -> R) -> R {
    operation(&mut InstalledPipeline { memory: &mut NativeMemory, io: &mut NativeIo, channel_index_residue: residue })
}
pub struct NativePipeline { pub channel_index_residue: u32 }
impl NativePipeline { pub const fn continuation() -> Self { Self { channel_index_residue: 0 } } }
impl Pipeline for NativePipeline {
    fn get(&self, field: Field) -> u32 { installed(self.channel_index_residue, |pipeline| pipeline.get(field)) }
    fn set(&mut self, field: Field, value: u32) { installed(self.channel_index_residue, |pipeline| pipeline.set(field, value)); }
    fn request(&self, field: RequestField) -> u32 { installed(self.channel_index_residue, |pipeline| pipeline.request(field)) }
    fn set_error(&mut self, error: u32) { installed(self.channel_index_residue, |pipeline| pipeline.set_error(error)); }
    fn selected_bank(&self) -> Bank { installed(self.channel_index_residue, |pipeline| pipeline.selected_bank()) }
    fn channel_index(&mut self) -> u32 { installed(self.channel_index_residue, |pipeline| pipeline.channel_index()) }
    fn program(&mut self) -> ServiceStatus { installed(self.channel_index_residue, |pipeline| pipeline.program()) }
    fn start_dma(&mut self) -> ServiceStatus { installed(self.channel_index_residue, |pipeline| pipeline.start_dma()) }
    fn dma_pending(&mut self) -> bool { installed(self.channel_index_residue, |pipeline| pipeline.dma_pending()) }
    fn close_dma(&mut self) -> ServiceStatus { installed(self.channel_index_residue, |pipeline| pipeline.close_dma()) }
    fn start_resource(&mut self) -> ServiceStatus { installed(self.channel_index_residue, |pipeline| pipeline.start_resource()) }
    fn ready(&mut self) { installed(self.channel_index_residue, |pipeline| pipeline.ready()); }
    fn schedule_ready(&mut self) { installed(self.channel_index_residue, |pipeline| pipeline.schedule_ready()); }
    fn fill(&mut self, address: PcmAddress, length: ByteLength) { installed(self.channel_index_residue, |pipeline| pipeline.fill(address, length)); }
}
impl PipelineLifecycle for NativePipeline {
    type Guard = NativeGuard;
    fn stop(&mut self) -> ServiceStatus { installed(self.channel_index_residue, |pipeline| pipeline.stop()) }
    fn release(&mut self) -> ServiceStatus { installed(self.channel_index_residue, |pipeline| pipeline.release()) }
    fn finished(&mut self) { installed(self.channel_index_residue, |pipeline| pipeline.finished()); }
    fn enter(&mut self) -> NativeGuard { installed(self.channel_index_residue, |pipeline| pipeline.enter()) }
    fn leave(&mut self, guard: NativeGuard) { installed(self.channel_index_residue, |pipeline| pipeline.leave(guard)); }
}
pub unsafe fn complete(completion: Completion) { installed(0, |pipeline| super::complete(pipeline, completion)); }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_audio_pipeline_submit(event: u32, position: u32, length: u32, continuation: u32, channel_index_residue: u32) -> i32 {
    installed(channel_index_residue, |pipeline| {
        if continuation > 1 {
            if pipeline.get(Field::Event) != event || event == 0 { pipeline.set_error(2); return -20; }
            return failed(pipeline, 5).0;
        }
        submit(pipeline, EventHandle(event), PcmAddress(position), ByteLength(length), if continuation == 1 { Submission::Continuation } else { Submission::First }).0
    })
}
#[no_mangle]
pub extern "C" fn xm1_power_audio_pipeline_update() -> i32 { installed(0, |pipeline| update(pipeline).0) }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_audio_pipeline_cancel(event: u32) -> i32 { installed(0, |pipeline| super::cancel(pipeline, EventHandle(event)).0) }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_audio_pipeline_release() -> i32 { installed(0, |pipeline| pipeline.release().0) }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_audio_stream_receive_complete() { complete(Completion::Receive); }
#[no_mangle]
pub unsafe extern "C" fn xm1_power_audio_stream_send_complete() { complete(Completion::Send); }
