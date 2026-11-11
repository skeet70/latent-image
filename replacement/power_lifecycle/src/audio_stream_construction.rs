use crate::primitives::ServiceStatus;
use super::stream_pipeline::Bank;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Address(pub u32);
impl Address { pub const fn offset(self, offset: u32) -> Self { Self(self.0.wrapping_add(offset)) } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemaphoreHandle(pub i32);
pub trait ConstructionMemory {
    fn word(&self, address: Address) -> u32;
    fn put(&mut self, address: Address, value: u32);
    fn byte(&self, address: Address) -> u8;
    fn put_byte(&mut self, address: Address, value: u8);
    fn half(&self, address: Address) -> u16 { u16::from_le_bytes([self.byte(address), self.byte(address.offset(1))]) }
    fn put_half(&mut self, address: Address, value: u16) {
        let bytes = value.to_le_bytes();
        self.put_byte(address, bytes[0]);
        self.put_byte(address.offset(1), bytes[1]);
    }
}
pub trait ConstructionIo {
    type Guard;
    fn enter(&mut self) -> Self::Guard;
    fn leave(&mut self, guard: Self::Guard);
    fn wait(&mut self, semaphore: SemaphoreHandle) -> ServiceStatus;
}
const PIPELINE: Address = Address(0x93b5f8);
const RESOURCE: Address = Address(0x973ac4);
const DMA: Address = Address(0x918b98);
const CONTROLLER: Address = Address(0x942d54);
const SERIAL: Address = Address(0x409c2804);
fn allocate(memory: &mut impl ConstructionMemory, index: u32, count: u32, handles: Address) -> ServiceStatus {
    if index >= count { return ServiceStatus(-30); }
    if memory.word(handles.offset(index * 4)) != 0 { return ServiceStatus(-20); }
    loop {
        memory.put(SERIAL, memory.word(SERIAL).wrapping_add(1));
        if memory.word(SERIAL) == 0 { memory.put(SERIAL, memory.word(SERIAL).wrapping_add(1)); }
        if (0..count).all(|other| memory.word(handles.offset(other * 4)) != memory.word(SERIAL)) {
            memory.put(handles.offset(index * 4), memory.word(SERIAL));
            return ServiceStatus(0);
        }
    }
}
pub fn controller_acquire(memory: &mut impl ConstructionMemory, io: &mut impl ConstructionIo, bank: Bank, output: Address) -> ServiceStatus {
    let saved = io.enter();
    let status = allocate(memory, bank.index(), 2, CONTROLLER);
    io.leave(saved);
    if status.0 != 0 { return ServiceStatus(-20); }
    memory.put(output, memory.word(CONTROLLER.offset(bank.index() * 4)));
    ServiceStatus(0)
}
pub fn resource_acquire(memory: &mut impl ConstructionMemory, io: &mut impl ConstructionIo, configuration: Address) -> ServiceStatus {
    let selected = Address(memory.word(Address(0x9566bc)));
    let bank = if memory.word(selected.offset(16)) == 3 { Bank::Second } else { Bank::First };
    let status = controller_acquire(memory, io, bank, RESOURCE.offset(72));
    if status.0 != 0 { memory.put(RESOURCE.offset(24), memory.word(RESOURCE.offset(24)) | 0x80); return status; }
    memory.put(RESOURCE.offset(24), memory.word(RESOURCE.offset(24)) | 2);
    for index in 0..24 { memory.put_byte(RESOURCE.offset(index), memory.byte(configuration.offset(index))); }
    let mut status = ServiceStatus(0);
    match memory.word(configuration) { 1 => memory.put(RESOURCE.offset(76), 0), 0 => memory.put(RESOURCE.offset(76), 1), _ => status = ServiceStatus(-30) }
    match memory.word(configuration.offset(12)) { 0 => memory.put(RESOURCE.offset(80), 0), 1 => memory.put(RESOURCE.offset(80), 1), _ => status = ServiceStatus(-30) }
    match memory.word(configuration.offset(16)) {
        0 => { memory.put(RESOURCE.offset(84), 0); memory.put(RESOURCE.offset(88), 0); },
        1 => { memory.put(RESOURCE.offset(84), 1); memory.put(RESOURCE.offset(88), 0); },
        _ => status = ServiceStatus(-30),
    }
    status
}
pub fn resource_configure(memory: &mut impl ConstructionMemory) -> ServiceStatus {
    let handle = memory.word(RESOURCE.offset(72));
    let status = if handle == 0 { ServiceStatus(-20) } else if let Some(index) = (0..2).find(|&index| memory.word(CONTROLLER.offset(index * 4)) == handle) {
        let base = Address(0xfffa0000 + index * 0x1000);
        let option = memory.word(RESOURCE.offset(80));
        let enabled = memory.word(RESOURCE.offset(84));
        let packing = memory.word(RESOURCE.offset(88));
        memory.put(base.offset(12), memory.word(base.offset(12)) & !1 | option & 1);
        memory.put(base.offset(12), memory.word(base.offset(12)) & !0x10000 | (enabled << 16) & 0x10000);
        memory.put(Address(0x95dbe8), memory.word(base.offset(12)));
        memory.put(Address(0x95dbe4), memory.word(base.offset(8)));
        memory.put(base.offset(8), memory.word(base.offset(8)) & !3 | 2);
        memory.put(base.offset(8), memory.word(base.offset(8)) & !0x8000);
        memory.put(Address(0x95dbe4), memory.word(base.offset(8)));
        memory.put(base.offset(0x100), packing & 3);
        memory.put(Address(0x95dbfc), memory.word(base.offset(0x100)));
        if memory.word(Address(0x95dbe8)) & 1 != option || memory.word(Address(0x95dbe8)) >> 16 & 1 != enabled || memory.word(Address(0x95dbe4)) & 3 != 2 || memory.word(Address(0x95dbe4)) >> 15 & 1 != 0 || memory.word(Address(0x95dbfc)) & 3 != packing { ServiceStatus(-255) } else { ServiceStatus(0) }
    } else { ServiceStatus(-20) };
    memory.put(RESOURCE.offset(28), memory.word(RESOURCE.offset(28)) | if status.0 == 0 { 2 } else { 0x80 });
    status
}
pub fn pipeline_acquire(memory: &mut impl ConstructionMemory, io: &mut impl ConstructionIo, request: Address, output: Address) -> ServiceStatus {
    memory.put(PIPELINE, request.0);
    memory.put(PIPELINE.offset(8), 0);
    let mut status = if memory.word(PIPELINE.offset(16)) == 0 { allocate(memory, 0, 1, PIPELINE.offset(16)) } else { ServiceStatus(-20) };
    if status.0 == 0 {
        memory.put(PIPELINE.offset(8), memory.word(PIPELINE.offset(8)) | 1);
        let channel = memory.word(Address(0x7c86bc + 2 * 4));
        let semaphore = channel + 11;
        let identifier = SemaphoreHandle((semaphore | memory.word(Address(0x93e6f4 + (semaphore - 1) * 4)) << 16) as i32);
        status = if io.wait(identifier).0 != 0 { ServiceStatus(-255) } else {
            let saved = io.enter();
            let allocated = allocate(memory, channel, 8, DMA.offset(64));
            io.leave(saved);
            if allocated.0 != 0 { ServiceStatus(-20) } else {
                memory.put(request.offset(20), memory.word(DMA.offset(64 + channel * 4)));
                memory.put(DMA.offset(channel * 4), 1);
                memory.put(DMA.offset(32 + channel * 4), 2);
                ServiceStatus(0)
            }
        };
        if status.0 == 0 {
            memory.put(RESOURCE.offset(68), memory.word(request.offset(20)));
            memory.put(RESOURCE.offset(24), memory.word(RESOURCE.offset(24)) | 4);
            memory.put(PIPELINE.offset(8), memory.word(PIPELINE.offset(8)) | 2);
            memory.put(PIPELINE.offset(20), memory.word(request.offset(20)));
        } else { memory.put(RESOURCE.offset(24), memory.word(RESOURCE.offset(24)) | 0x80); }
    }
    if status.0 != 0 { memory.put(PIPELINE.offset(4), 8); memory.put(request.offset(32), 2); return status; }
    memory.put(output, memory.word(PIPELINE.offset(16)));
    memory.put(PIPELINE.offset(4), 1);
    memory.put(request.offset(32), 0);
    ServiceStatus(0)
}
pub fn resource_release(memory: &mut impl ConstructionMemory) -> ServiceStatus {
    let handle = memory.word(RESOURCE.offset(72));
    let status = if handle == 0 { ServiceStatus(-20) } else if let Some(index) = (0..2).find(|&index| memory.word(CONTROLLER.offset(index * 4)) == handle) {
        memory.put(CONTROLLER.offset(index * 4), 0); ServiceStatus(0)
    } else { ServiceStatus(-20) };
    memory.put(RESOURCE.offset(24), if status.0 == 0 { memory.word(RESOURCE.offset(24)) & !2 } else { memory.word(RESOURCE.offset(24)) | 0x80 });
    status
}

#[cfg(target_arch = "arm")]
#[path = "audio_stream_construction_native.rs"]
pub mod native;
#[cfg(test)]
#[path = "audio_stream_construction_tests.rs"]
mod tests;
