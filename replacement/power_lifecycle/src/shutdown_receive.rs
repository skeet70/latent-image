use crate::shutdown::{KernelStatus, QueueId, RequestId, TaskId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceiveTimeout { Forever, Poll, Ticks(core::num::NonZeroU32) }
impl ReceiveTimeout {
    pub const fn word(self) -> i32 { match self { Self::Forever => -1, Self::Poll => 0, Self::Ticks(value) => value.get() as i32 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceivedHeader { pub operation: RequestId, pub sender: TaskId, pub destination: QueueId }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct PayloadAddress(pub u32);
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiveDescriptor {
    operation: u16,
    sender: u8,
    destination: u8,
    length: u32,
    pub payload: PayloadAddress,
}
impl ReceiveDescriptor {
    pub const fn new(payload: PayloadAddress) -> Self { Self { operation: 0, sender: 0, destination: 0, length: 0, payload } }
    pub const fn header(&self) -> ReceivedHeader { ReceivedHeader { operation: RequestId(self.operation), sender: TaskId(self.sender), destination: QueueId(self.destination) } }
    pub const fn length(&self) -> u32 { self.length }
}
pub trait ReceiveKernel {
    type Block: Copy;
    fn receive(&mut self, queue: QueueId, timeout: ReceiveTimeout) -> Result<Self::Block, KernelStatus>;
    fn header(&self, block: Self::Block) -> (ReceivedHeader, u32);
    unsafe fn copy_payload(&mut self, block: Self::Block, destination: PayloadAddress, length: u32);
    fn release(&mut self, queue: QueueId, block: Self::Block);
    fn report(&mut self, queue: QueueId, status: KernelStatus);
}
/// The descriptor is unchanged when receive fails; its payload address is never replaced.
///
/// # Safety
/// The descriptor payload address must hold the full payload of every received message.
pub unsafe fn receive_into(kernel: &mut impl ReceiveKernel, queue: QueueId, descriptor: &mut ReceiveDescriptor, timeout: ReceiveTimeout) -> Result<(), KernelStatus> {
    let block = match kernel.receive(queue, timeout) {
        Ok(block) => block,
        Err(status) => { if status.0 != -50 { kernel.report(queue, status); } return Err(status); }
    };
    let (header, length) = kernel.header(block);
    descriptor.operation = header.operation.0;
    descriptor.sender = header.sender.0;
    descriptor.destination = header.destination.0;
    descriptor.length = length;
    if length != 0 { unsafe { kernel.copy_payload(block, descriptor.payload, length) }; }
    kernel.release(queue, block);
    Ok(())
}

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use crate::callback_registry::kernel_slot;
    use crate::shutdown_rtos::NativeShutdownRtos;
    use core::ptr::{read_volatile, write_volatile};
    extern "C" { fn xm1_power_service_error(operation: u32, id: i32, status: i32); }
    unsafe fn word(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
    unsafe fn root() -> u32 {
        let primary = unsafe { word(0x96bd20) };
        let core: unsafe extern "C" fn() -> u32 = unsafe { kernel_slot(primary, 0x1618) };
        unsafe { word(0x96bd20 + core() * 4) }
    }
    unsafe fn encoded(queue: QueueId) -> i32 { let slot = u32::from(queue.0); (slot | unsafe { word(0x7cad9c + (slot - 1) * 4) } << 16) as i32 }
    impl ReceiveKernel for NativeShutdownRtos {
        type Block = u32;
        fn receive(&mut self, queue: QueueId, timeout: ReceiveTimeout) -> Result<u32, KernelStatus> {
            let root = unsafe { root() };
            let call: unsafe extern "C" fn(i32, *mut u32, i32) -> i32 = unsafe { kernel_slot(root, 0xc8) };
            let mut block = 0;
            let status = unsafe { call(encoded(queue), &mut block, timeout.word()) };
            if status == 0 { Ok(block) } else { Err(KernelStatus(status)) }
        }
        fn header(&self, block: u32) -> (ReceivedHeader, u32) { unsafe {
            (ReceivedHeader { operation: RequestId(read_volatile((block + 4) as *const u16)), sender: TaskId(read_volatile((block + 6) as *const u8)), destination: QueueId(read_volatile((block + 7) as *const u8)) }, word(block + 8))
        } }
        unsafe fn copy_payload(&mut self, block: u32, destination: PayloadAddress, length: u32) { unsafe { core::ptr::copy_nonoverlapping((block + 12) as *const u8, destination.0 as *mut u8, length as usize) }; }
        fn release(&mut self, queue: QueueId, block: u32) {
            let call: unsafe extern "C" fn(i32, u32) -> i32 = unsafe { kernel_slot(unsafe { root() }, 0xf8) };
            let _ = unsafe { call(encoded(queue), block) };
        }
        fn report(&mut self, queue: QueueId, status: KernelStatus) { unsafe {
            write_volatile(0x40a0f82c as *mut i32, status.0);
            xm1_power_service_error(31, i32::from(queue.0), status.0);
        } }
    }
    impl NativeShutdownRtos {
        /// # Safety
        /// The descriptor payload must accommodate each incoming message.
        pub unsafe fn receive(&mut self, queue: QueueId, descriptor: &mut ReceiveDescriptor, timeout: ReceiveTimeout) -> Result<(), KernelStatus> { unsafe { receive_into(self, queue, descriptor, timeout) } }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_queue_receive(queue: u32, descriptor: *mut ReceiveDescriptor, timeout: i32) -> i32 {
        let timeout = match timeout { -1 => ReceiveTimeout::Forever, 0 => ReceiveTimeout::Poll, value => ReceiveTimeout::Ticks(core::num::NonZeroU32::new(value as u32).unwrap()) };
        match unsafe { NativeShutdownRtos.receive(QueueId(queue as u8), &mut *descriptor, timeout) } { Ok(()) => 0, Err(status) => status.0 }
    }
}

#[cfg(test)]
#[path = "shutdown_receive_tests.rs"]
mod tests;
