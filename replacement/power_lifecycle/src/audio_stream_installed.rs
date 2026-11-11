use super::executor::NodeAddress;
use super::stream::{ByteLength, PcmAddress};
use super::stream_construction::{Address, ConstructionIo, ConstructionMemory, SemaphoreHandle};
use super::stream_pipeline::{Bank, Completion, Field, Pipeline, PipelineLifecycle, RequestField};
use crate::primitives::ServiceStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrqSource(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrqHandle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackAddress(pub u32);
pub trait InstalledIo: ConstructionIo {
    type IrqMask;
    fn save_irq(&mut self) -> Self::IrqMask;
    fn restore_irq(&mut self, saved: Self::IrqMask);
    fn attach_irq(&mut self, memory: &mut impl ConstructionMemory, source: IrqSource, completion: Completion, output: Address) -> ServiceStatus;
    fn release_irq(&mut self, handle: IrqHandle) -> ServiceStatus;
    fn clean_data_cache(&mut self);
    fn signal(&mut self, semaphore: SemaphoreHandle) -> ServiceStatus;
    fn invoke(&mut self, memory: &mut impl ConstructionMemory, callback: CallbackAddress);
    fn schedule(&mut self, memory: &mut impl ConstructionMemory, node: NodeAddress, output: Address) -> ServiceStatus;
}
pub struct InstalledPipeline<'a, M, I> {
    pub memory: &'a mut M,
    pub io: &'a mut I,
    pub channel_index_residue: u32,
}
const STATE: Address = Address(0x93b5f8);
const DMA: Address = Address(0x918b98);
const RESOURCE: Address = Address(0x973ac4);
impl<M: ConstructionMemory, I: InstalledIo> InstalledPipeline<'_, M, I> {
    fn word(&self, address: Address) -> u32 { self.memory.word(address) }
    fn put(&mut self, address: Address, value: u32) { self.memory.put(address, value); }
    fn find_handle(&self, handle: u32, base: Address, count: u32) -> Option<u32> {
        if handle == 0 { None } else { (0..count).find(|&index| self.word(base.offset(index * 4)) == handle) }
    }
    fn channel(&self) -> Option<u32> { self.find_handle(self.get(Field::Transfer), DMA.offset(64), 8) }
    fn invoke_address(&mut self, address: u32) { if address != 0 { self.io.invoke(self.memory, CallbackAddress(address)); } }
    fn close_handle(&mut self, handle: u32) -> ServiceStatus {
        let Some(channel) = self.find_handle(handle, DMA.offset(64), 8) else { return ServiceStatus(-20); };
        let slot = DMA.offset(96 + channel * 4);
        let mut status = ServiceStatus(0);
        if self.word(slot) != 0 { status = self.io.release_irq(IrqHandle(self.word(slot))); self.put(slot, 0); }
        self.put(DMA.offset(32 + channel * 4), u32::MAX);
        self.put(DMA.offset(channel * 4), 0);
        self.put(DMA.offset(64 + channel * 4), 0);
        let semaphore = channel + 11;
        let identifier = SemaphoreHandle((semaphore | self.word(Address(0x93e6f4 + (semaphore - 1) * 4)) << 16) as i32);
        if self.io.signal(identifier).0 != 0 { return ServiceStatus(-255); }
        status
    }
    fn stop_dma(&mut self) -> ServiceStatus {
        let Some(channel) = self.channel() else { return ServiceStatus(-20); };
        if self.word(DMA.offset(channel * 4)) != 2 { return ServiceStatus(-11); }
        while self.word(Address(0xfff02024 + channel * 64)) & 1 != 0 { core::hint::spin_loop(); }
        let slot = DMA.offset(96 + channel * 4);
        let status = if self.word(slot) == 0 { ServiceStatus(0) } else {
            let status = self.io.release_irq(IrqHandle(self.word(slot)));
            self.put(slot, 0);
            status
        };
        self.put(DMA.offset(channel * 4), 1);
        status
    }
    fn stop_resource(&mut self) -> ServiceStatus {
        if self.word(RESOURCE.offset(28)) & 4 == 0 { return ServiceStatus(0); }
        let handle = self.word(RESOURCE.offset(72));
        let Some(index) = self.find_handle(handle, Address(0x942d54), 2) else {
            self.put(RESOURCE.offset(28), self.word(RESOURCE.offset(28)) | 0x80);
            return ServiceStatus(-20);
        };
        let base = Address(0xfffa0000 + index * 0x1000);
        if self.word(RESOURCE.offset(76)) == 0 {
            for _ in 0..0xffff {
                let status = self.word(base.offset(80)); self.put(Address(0x95dbf0), status);
                if status & 2 == 0 { break; }
            }
        }
        self.put(Address(0x95dbe4), self.word(base.offset(8)));
        let mode = self.word(Address(0x95dbe4));
        if mode & 0x20000 != 0 { self.put(base.offset(8), self.word(base.offset(8)) & !0x20000); }
        if mode & 0x10000 != 0 { self.put(base.offset(8), self.word(base.offset(8)) & !0x10000); }
        self.put(Address(0x95dbe4), self.word(base.offset(8)));
        if self.word(Address(0x942d5c)) != 0 {
            self.put(base.offset(64), 0);
            self.put(Address(0x95dbec), self.word(base.offset(64)));
            let _ = self.io.release_irq(IrqHandle(self.word(Address(0x942d5c))));
            self.put(Address(0x942d5c), 0);
        }
        self.put(RESOURCE.offset(28), self.word(RESOURCE.offset(28)) & !4);
        ServiceStatus(0)
    }
}
impl<M: ConstructionMemory, I: InstalledIo> Pipeline for InstalledPipeline<'_, M, I> {
    fn get(&self, field: Field) -> u32 { self.word(STATE.offset(field as u32)) }
    fn set(&mut self, field: Field, value: u32) { self.put(STATE.offset(field as u32), value); }
    fn request(&self, field: RequestField) -> u32 { self.word(Address(self.get(Field::Request)).offset(field.offset())) }
    fn set_error(&mut self, error: u32) { self.put(Address(self.get(Field::Request)).offset(32), error); }
    fn selected_bank(&self) -> Bank { if self.word(Address(self.word(Address(0x9566bc))).offset(16)) == 3 { Bank::Second } else { Bank::First } }
    fn channel_index(&mut self) -> u32 { self.channel().unwrap_or(self.channel_index_residue) }
    fn program(&mut self) -> ServiceStatus {
        let Some(channel) = self.channel() else { return ServiceStatus(-20); };
        let mut control = self.get(Field::Control);
        if control & 0x30 != 0 {
            let at = Address(0xfff98500 + (channel / 4) * 4);
            let shift = (channel % 4) * 8;
            let saved = self.io.save_irq();
            self.put(at, self.word(at) & !(0x1f << shift) | self.get(Field::Priority).wrapping_shl(shift));
            self.io.restore_irq(saved);
            control = control & !7 | channel;
        }
        let at = Address(0xfff02000 + channel * 64);
        self.put(at.offset(44), control);
        if control & 0x80000000 != 0 { self.put(at.offset(56), self.word(STATE.offset(88)) & !3); }
        else {
            for (register, field) in [(0, 64), (4, 68), (8, 72), (12, 76), (16, 80), (20, 84), (48, 60), (52, 56)] {
                self.put(at.offset(register), self.word(STATE.offset(field)));
            }
        }
        ServiceStatus(0)
    }
    fn start_dma(&mut self) -> ServiceStatus {
        let Some(channel) = self.channel() else { return ServiceStatus(-20); };
        let at = Address(0xfff02000 + channel * 64);
        if self.word(DMA.offset(channel * 4)) != 1 || self.word(at.offset(36)) & 5 != 0 { return ServiceStatus(-12); }
        if self.get(Field::Callback) != 0 {
            let Some(completion) = Completion::from_abi(self.get(Field::Callback)) else { return ServiceStatus(-255); };
            let source = IrqSource(self.get(Field::Argument));
            let status = self.io.attach_irq(self.memory, source, completion, DMA.offset(96 + channel * 4));
            if status.0 != 0 { return status; }
        }
        self.put(DMA.offset(channel * 4), 2);
        self.put(at.offset(40), 8);
        let interrupt = if self.word(at.offset(44)) & 0x30 == 0 { 4 } else { 0 };
        self.io.clean_data_cache();
        self.put(at.offset(40), interrupt | 1);
        ServiceStatus(0)
    }
    fn dma_pending(&mut self) -> bool { self.channel().is_some_and(|channel| self.word(Address(0xfff02024 + channel * 64)) & 4 != 0) }
    fn close_dma(&mut self) -> ServiceStatus { self.close_handle(self.get(Field::Transfer)) }
    fn start_resource(&mut self) -> ServiceStatus {
        let handle = self.word(RESOURCE.offset(72));
        let status = match self.find_handle(handle, Address(0x942d54), 2) {
            None => ServiceStatus(-20),
            Some(index) => {
                let at = Address(0xfffa0008 + index * 0x1000);
                let receive = self.word(RESOURCE.offset(76)) != 0;
                let (enabled, opposite) = if receive { (0x10000, 0x20000) } else { (0x20000, 0x10000) };
                self.put(Address(0x95dbe4), self.word(at));
                if self.word(Address(0x95dbe4)) & opposite != 0 { ServiceStatus(-12) }
                else { self.put(at, self.word(at) | enabled); self.put(Address(0x95dbe4), self.word(at)); ServiceStatus(0) }
            }
        };
        self.put(RESOURCE.offset(28), self.word(RESOURCE.offset(28)) | if status.0 == 0 { 4 } else { 0x80 });
        status
    }
    fn ready(&mut self) { self.invoke_address(self.request(RequestField::Ready)); }
    fn schedule_ready(&mut self) { let _ = self.io.schedule(self.memory, NodeAddress(0x7d08a8), STATE.offset(24)); }
    fn fill(&mut self, address: PcmAddress, length: ByteLength) {
        for offset in 0..length.0 { self.memory.put_byte(Address(address.0.wrapping_add(offset)), 0x80); }
    }
}
impl<M: ConstructionMemory, I: InstalledIo> PipelineLifecycle for InstalledPipeline<'_, M, I> {
    type Guard = I::Guard;
    fn stop(&mut self) -> ServiceStatus {
        let mut previous = ServiceStatus(0);
        if self.get(Field::Flags) & 2 != 0 {
            previous = self.stop_dma();
            if previous.0 == 0 { self.set(Field::Flags, self.get(Field::Flags) & !2); }
        }
        let status = self.stop_resource();
        if status.0 == 0 { previous } else { status }
    }
    fn release(&mut self) -> ServiceStatus {
        self.set(Field::Timer, 0);
        let status = self.close_handle(self.word(RESOURCE.offset(68)));
        if status.0 == 0 {
            self.put(RESOURCE.offset(24), self.word(RESOURCE.offset(24)) & !4);
            self.put(STATE.offset(8), self.word(STATE.offset(8)) & !2);
        } else { self.put(RESOURCE.offset(24), self.word(RESOURCE.offset(24)) | 0x80); }
        if self.get(Field::Event) == 0 { return ServiceStatus(0); }
        self.set(Field::Event, 0);
        self.put(STATE.offset(8), self.word(STATE.offset(8)) & !1);
        ServiceStatus(0)
    }
    fn finished(&mut self) { self.invoke_address(self.request(RequestField::Finished)); }
    fn enter(&mut self) -> Self::Guard { self.io.enter() }
    fn leave(&mut self, guard: Self::Guard) { self.io.leave(guard); }
}
