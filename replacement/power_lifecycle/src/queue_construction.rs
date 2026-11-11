use crate::primitives::{QueueId, ServiceStatus, StorageSlot};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct QueueConfiguration { pub flags: u32, pub message_size: u32 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ServiceAddress(pub u32);
#[repr(C)]
pub struct DataQueueConfiguration { pub attributes: u32, pub capacity: u32, pub ring: ServiceAddress }
#[repr(C)]
pub struct PoolConfiguration { pub attributes: u32, pub blocks: u32, pub block_size: u32, pub buffer: ServiceAddress }
#[derive(Clone, Copy)]
pub enum ConstructionDiagnostic { Storage, DataQueue, Pool }
impl ConstructionDiagnostic {
    pub const fn operation(self) -> u32 { match self { Self::Storage => 0x25, Self::DataQueue => 0x30, Self::Pool => 0x22 } }
}
pub trait QueueConstruction {
    fn reserve(&mut self, slot: StorageSlot, bytes: u32) -> Result<ServiceAddress, ServiceStatus>;
    fn release(&mut self, slot: StorageSlot);
    fn publish_ring(&mut self, queue: QueueId, address: ServiceAddress);
    fn publish_pool(&mut self, queue: QueueId, message_size: u32, bytes: u32, address: ServiceAddress);
    fn clear_pool(&mut self, queue: QueueId);
    fn create_data_queue(&mut self, queue: QueueId, configuration: &DataQueueConfiguration) -> ServiceStatus;
    fn delete_data_queue(&mut self, queue: QueueId);
    fn create_pool(&mut self, queue: QueueId, configuration: &PoolConfiguration) -> ServiceStatus;
    fn error(&mut self, operation: ConstructionDiagnostic, channel: u32, status: ServiceStatus);
}
pub fn create_queue(kernel: &mut impl QueueConstruction, channel: u32, configuration: QueueConfiguration) -> ServiceStatus {
    let Some(queue) = QueueId::from_abi(channel) else { return ServiceStatus(-18); };
    let ring_slot = StorageSlot(queue.slot() + 0x77);
    let pool_slot = StorageSlot(queue.slot() - 1);
    let ring = match kernel.reserve(ring_slot, configuration.flags.wrapping_mul(4)) {
        Ok(address) => address,
        Err(status) => { kernel.error(ConstructionDiagnostic::Storage, channel, status); return status; }
    };
    kernel.publish_ring(queue, ring);
    let status = kernel.create_data_queue(queue, &DataQueueConfiguration { attributes: 0, capacity: configuration.flags, ring });
    if status.0 != 0 {
        kernel.release(ring_slot);
        kernel.error(ConstructionDiagnostic::DataQueue, channel, status);
        return status;
    }
    let block_size = configuration.message_size.wrapping_add(4);
    let bytes = configuration.flags.wrapping_mul(block_size);
    let buffer = match kernel.reserve(pool_slot, bytes) {
        Ok(address) => address,
        Err(status) => {
            kernel.delete_data_queue(queue);
            kernel.release(ring_slot);
            kernel.clear_pool(queue);
            kernel.error(ConstructionDiagnostic::Storage, channel, status);
            return status;
        }
    };
    kernel.publish_pool(queue, configuration.message_size, bytes, buffer);
    let status = kernel.create_pool(queue, &PoolConfiguration { attributes: 0, blocks: configuration.flags, block_size, buffer });
    if status.0 != 0 {
        kernel.delete_data_queue(queue);
        kernel.release(pool_slot);
        kernel.release(ring_slot);
        kernel.clear_pool(queue);
        kernel.error(ConstructionDiagnostic::Pool, channel, status);
    }
    status
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use crate::primitives::{CpuCore, NativeQueueStorage, OwnedQueueRtos, QueueStorage, destroy_queue};
    use crate::diagnostics::NativeQueueDiagnostics;
    use core::ptr::{read_volatile, write_volatile};
    extern "C" {
        fn xm1_rtos_own_first_slot_1618() -> u32;
        fn xm1_power_save_irq() -> u32;
        fn xm1_power_restore_irq(saved: u32);
        fn helper_try(word: u32, stamp: u32) -> i32;
        fn helper_release(word: u32) -> i32;
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(saved: u32);
        fn xm1_rtos_data_queue_create_body(id: i32, packet: u32) -> i32;
        fn xm1_rtos_data_queue_delete_body(id: i32) -> i32;
        fn xm1_rtos_fixed_pool_create_body(id: u32, packet: u32) -> i32;
        fn xm1_power_service_error(operation: u32, id: i32, status: i32);
    }
    unsafe fn word(address: u32) -> u32 { read_volatile(address as *const u32) }
    unsafe fn put(address: u32, value: u32) { write_volatile(address as *mut u32, value); }
    fn current_core() -> CpuCore { if unsafe { xm1_rtos_own_first_slot_1618() } == 0 { CpuCore::First } else { CpuCore::Second } }
    fn storage() -> NativeQueueStorage<NativeQueueDiagnostics> { NativeQueueStorage { core: current_core(), diagnostics: NativeQueueDiagnostics } }
    fn encoded(queue: QueueId) -> u32 { queue.slot() | unsafe { word(0x7cad9c + (queue.slot() - 1) * 4) } << 16 }
    fn pool_metadata(queue: QueueId) -> u32 { 0x409c3500 + (queue.slot() - 1 + 0x9600) * 8 + 0xf64 }
    struct NativeConstruction;
    impl QueueConstruction for NativeConstruction {
        fn reserve(&mut self, slot: StorageSlot, bytes: u32) -> Result<ServiceAddress, ServiceStatus> {
            if slot.0 >= 240 { return Err(ServiceStatus(-18)); }
            unsafe {
                let core = current_core();
                let saved = xm1_power_save_irq();
                let _ = helper_try(0x409c280c, core.lock_stamp());
                let record = 0x409c3904 + slot.0 * 12;
                let previous = word(record);
                let result = if previous != 0 {
                    if word(record + 4) != bytes { Err(ServiceStatus(-33)) }
                    else if word(record + 8) != 0 { Err(ServiceStatus(-18)) }
                    else { put(record + 8, 1); Ok(ServiceAddress(previous)) }
                } else {
                    let cursor = word(0x4089ea90);
                    if cursor.wrapping_sub(0x409c4460).wrapping_add(bytes) > 0x4b000 { Err(ServiceStatus(-33)) }
                    else {
                        put(0x4089ea90, cursor.wrapping_add(bytes & !3));
                        put(record, cursor);
                        put(record + 4, bytes);
                        put(record + 8, 1);
                        Ok(ServiceAddress(cursor))
                    }
                };
                let _ = helper_release(0x409c280c);
                xm1_power_restore_irq(saved);
                result
            }
        }
        fn release(&mut self, slot: StorageSlot) { let _ = storage().release_storage(slot); }
        fn publish_ring(&mut self, queue: QueueId, address: ServiceAddress) { unsafe { put(0x40a0f870 + (queue.slot() - 1) * 4, address.0); } }
        fn publish_pool(&mut self, queue: QueueId, message_size: u32, bytes: u32, address: ServiceAddress) {
            unsafe { put(0x40a0fa50 + (queue.slot() - 1) * 4, message_size); put(pool_metadata(queue), address.0); put(pool_metadata(queue) + 4, bytes); }
        }
        fn clear_pool(&mut self, queue: QueueId) { storage().clear_active(queue); }
        fn create_data_queue(&mut self, queue: QueueId, configuration: &DataQueueConfiguration) -> ServiceStatus {
            unsafe { let saved = xm1_power_interrupt_lock(); let status = xm1_rtos_data_queue_create_body(encoded(queue) as i32, configuration as *const _ as u32); xm1_power_interrupt_unlock(saved); ServiceStatus(status) }
        }
        fn delete_data_queue(&mut self, queue: QueueId) { unsafe { let _ = xm1_rtos_data_queue_delete_body(encoded(queue) as i32); } }
        fn create_pool(&mut self, queue: QueueId, configuration: &PoolConfiguration) -> ServiceStatus {
            unsafe { let saved = xm1_power_interrupt_lock(); let status = xm1_rtos_fixed_pool_create_body(encoded(queue), configuration as *const _ as u32); xm1_power_interrupt_unlock(saved); ServiceStatus(status) }
        }
        fn error(&mut self, operation: ConstructionDiagnostic, channel: u32, status: ServiceStatus) { unsafe { xm1_power_service_error(operation.operation(), channel as i32, status.0); } }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_queue_create(channel: u32, configuration: *const QueueConfiguration) -> i32 { create_queue(&mut NativeConstruction, channel, configuration.read()).0 }
    #[no_mangle]
    pub extern "C" fn xm1_power_queue_delete(channel: u32) -> i32 { destroy_queue(&mut OwnedQueueRtos, &mut storage(), channel).0 }
}

#[cfg(test)]
#[path = "queue_construction_tests.rs"]
mod tests;
