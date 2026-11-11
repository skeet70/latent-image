use crate::standby::SavedIrq;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct QueueId(u32);
impl QueueId {
    pub fn from_abi(value: u32) -> Option<Self> {
        let slot = value & 0xffff;
        if (1..=120).contains(&slot) { Some(Self(slot)) } else { None }
    }
    #[cfg(target_arch = "arm")]
    fn encoded(self) -> u32 {
        let class = unsafe { core::ptr::read_volatile((0x7cad9c + (self.slot() - 1) * 4) as *const u32) };
        self.slot() | class << 16
    }
    pub const fn slot(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MailboxId;
impl MailboxId {
    pub fn from_abi(value: u32) -> Option<Self> { if value & 0xffff == 1 { Some(Self) } else { None } }
    pub const fn slot(self) -> u32 { 1 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct PoolBlock(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ServiceStatus(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReceivePoll { Message(PoolBlock), Stopped(ServiceStatus) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueDiagnostic { StorageRelease, Delete, InvalidIndex }
impl QueueDiagnostic {
    pub const fn operation(self) -> u32 { match self { Self::StorageRelease => 0x26, Self::Delete | Self::InvalidIndex => 0x1b } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct StorageSlot(pub u32);

pub trait QueueRtos {
    fn receive_poll(&mut self, queue: QueueId) -> ReceivePoll;
    fn release_block(&mut self, queue: QueueId, block: PoolBlock) -> ServiceStatus;
    fn delete_pool(&mut self, queue: QueueId) -> ServiceStatus;
    fn delete_queue(&mut self, queue: QueueId) -> ServiceStatus;
}
pub trait QueueStorage {
    fn release_storage(&mut self, slot: StorageSlot) -> ServiceStatus;
    fn clear_active(&mut self, queue: QueueId);
    fn diagnostic(&mut self, operation: QueueDiagnostic, channel: u32, status: ServiceStatus);
    fn record_error(&mut self, status: ServiceStatus);
}

pub fn destroy_queue(rtos: &mut impl QueueRtos, storage: &mut impl QueueStorage, channel: u32) -> ServiceStatus {
    let Some(queue) = QueueId::from_abi(channel) else {
        let status = ServiceStatus(-18);
        storage.diagnostic(QueueDiagnostic::InvalidIndex, channel, status);
        storage.record_error(status);
        return status;
    };
    while let ReceivePoll::Message(block) = rtos.receive_poll(queue) { let _ = rtos.release_block(queue, block); }
    let _ = rtos.delete_pool(queue);
    let first = storage.release_storage(StorageSlot(queue.slot() - 1));
    let second = storage.release_storage(StorageSlot(queue.slot() + 0x77));
    let combined = ServiceStatus(first.0 | second.0);
    if combined.0 != 0 { storage.diagnostic(QueueDiagnostic::StorageRelease, channel, combined); }
    storage.clear_active(queue);
    let status = rtos.delete_queue(queue);
    if status.0 != 0 {
        storage.diagnostic(QueueDiagnostic::Delete, channel, status);
        storage.record_error(status);
    }
    status
}

pub trait GpioHardware {
    fn write_port(&mut self, port: GpioPort, register: GpioRegister, value: u32);
    fn save_irq(&mut self) -> SavedIrq;
    fn clear_global_enable(&mut self);
    fn restore_irq(&mut self, saved: SavedIrq);
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpioPort(u8);
impl GpioPort { pub const fn index(self) -> u8 { self.0 } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GpioRegister { Interrupt, LowerMode, UpperMode, Mode08, Mode04, Output }
impl GpioRegister {
    pub const fn offset(self) -> u32 { match self { Self::Interrupt => 0x18, Self::LowerMode => 0x0c, Self::UpperMode => 0x10, Self::Mode08 => 8, Self::Mode04 => 4, Self::Output => 0 } }
}
pub fn reset_gpio(hardware: &mut impl GpioHardware) {
    for port in 0..16 {
        let port = GpioPort(port);
        hardware.write_port(port, GpioRegister::Interrupt, 0);
        for register in [GpioRegister::LowerMode, GpioRegister::UpperMode, GpioRegister::Mode08, GpioRegister::Mode04, GpioRegister::Output] {
            hardware.write_port(port, register, 0xffff0000);
        }
    }
    let saved = hardware.save_irq();
    hardware.clear_global_enable();
    hardware.restore_irq(saved);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerConfiguration { Mode, Pin1f, Pin20, Pin21 }
impl PowerConfiguration {
    pub const fn offset(self) -> u32 { match self { Self::Mode => 0x3ba, Self::Pin1f => 0x3b7, Self::Pin20 => 0x3b8, Self::Pin21 => 0x3b9 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerField { Mode22, Pin1f, Pin20, Pin21, Standby5, SystemRefresh7 }
impl PowerField {
    pub const fn index(self) -> u32 { match self { Self::Mode22 => 0x22, Self::Pin1f => 0x1f, Self::Pin20 => 0x20, Self::Pin21 => 0x21, Self::Standby5 => 5, Self::SystemRefresh7 => 7 } }
}
pub trait TerminalPower {
    fn read_configuration(&mut self, field: PowerConfiguration) -> u8;
    fn write_field(&mut self, field: PowerField, value: u8) -> ServiceStatus;
}
pub const fn power_ic_success() -> ServiceStatus { ServiceStatus(0) }
pub fn power_pin_write_f0(power: &mut impl TerminalPower) -> ServiceStatus {
    if power.read_configuration(PowerConfiguration::Mode) != 0 {
        let mode = power.read_configuration(PowerConfiguration::Mode) & 1;
        let _ = power.write_field(PowerField::Mode22, mode);
        for (configuration, field) in [(PowerConfiguration::Pin1f, PowerField::Pin1f), (PowerConfiguration::Pin20, PowerField::Pin20), (PowerConfiguration::Pin21, PowerField::Pin21)] {
            let value = power.read_configuration(configuration);
            let _ = power.write_field(field, value);
        }
    }
    let _ = power.write_field(PowerField::Standby5, 0xf0);
    ServiceStatus(0)
}
pub fn terminal(power: &mut impl TerminalPower, gpio: &mut impl GpioHardware) -> ! {
    let _ = power_ic_success();
    let _ = power_pin_write_f0(power);
    reset_gpio(gpio);
    loop { core::hint::spin_loop(); }
}

#[cfg(target_arch = "arm")]
pub struct OwnedQueueRtos;
#[cfg(target_arch = "arm")]
extern "C" {
    fn xm1_rtos_data_queue_receive_poll_body(id: i32, out: u32) -> i32;
    fn xm1_rtos_data_queue_delete_body(id: i32) -> i32;
    fn xm1_rtos_fixed_pool_release_body(id: i32, block: u32) -> i32;
    fn xm1_rtos_fixed_pool_delete_body(id: u32) -> i32;
}
#[cfg(target_arch = "arm")]
impl QueueRtos for OwnedQueueRtos {
    fn receive_poll(&mut self, queue: QueueId) -> ReceivePoll {
        let mut block = core::mem::MaybeUninit::<u32>::uninit();
        let status = unsafe { xm1_rtos_data_queue_receive_poll_body(queue.encoded() as i32, block.as_mut_ptr() as u32) };
        if status == 0 { ReceivePoll::Message(PoolBlock(unsafe { block.assume_init() })) } else { ReceivePoll::Stopped(ServiceStatus(status)) }
    }
    fn release_block(&mut self, queue: QueueId, block: PoolBlock) -> ServiceStatus { ServiceStatus(unsafe { xm1_rtos_fixed_pool_release_body(queue.encoded() as i32, block.0) }) }
    fn delete_pool(&mut self, queue: QueueId) -> ServiceStatus { ServiceStatus(unsafe { xm1_rtos_fixed_pool_delete_body(queue.encoded()) }) }
    fn delete_queue(&mut self, queue: QueueId) -> ServiceStatus { ServiceStatus(unsafe { xm1_rtos_data_queue_delete_body(queue.encoded() as i32) }) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CpuCore { First, Second }
impl CpuCore { pub const fn lock_stamp(self) -> u32 { match self { Self::First => 1, Self::Second => 2 } } }

pub trait QueueDiagnostics {
    fn report(&mut self, operation: QueueDiagnostic, channel: u32, status: ServiceStatus);
    fn dispatch_error(&mut self, status: ServiceStatus);
}

#[cfg(target_arch = "arm")]
pub struct NativeQueueStorage<D> { pub core: CpuCore, pub diagnostics: D }
#[cfg(target_arch = "arm")]
extern "C" {
    fn xm1_power_save_irq() -> u32;
    fn xm1_power_restore_irq(saved: u32);
    fn helper_try(word: u32, stamp: u32) -> i32;
    fn helper_release(word: u32) -> i32;
}
#[cfg(target_arch = "arm")]
impl<D: QueueDiagnostics> QueueStorage for NativeQueueStorage<D> {
    fn release_storage(&mut self, slot: StorageSlot) -> ServiceStatus {
        if slot.0 >= 240 { return ServiceStatus(-18); }
        unsafe {
            let irq = xm1_power_save_irq();
            let _ = helper_try(0x409c280c, self.core.lock_stamp());
            core::ptr::write_volatile((0x409c390c + slot.0 * 12) as *mut u32, 0);
            let _ = helper_release(0x409c280c);
            xm1_power_restore_irq(irq);
        }
        ServiceStatus(0)
    }
    fn clear_active(&mut self, queue: QueueId) {
        unsafe { core::ptr::write_volatile((0x409c3500 + (queue.slot() - 1 + 0x9600) * 8 + 0xf64) as *mut u32, 0) };
    }
    fn diagnostic(&mut self, operation: QueueDiagnostic, channel: u32, status: ServiceStatus) { self.diagnostics.report(operation, channel, status); }
    fn record_error(&mut self, status: ServiceStatus) {
        unsafe { core::ptr::write_volatile(0x40a0f82c as *mut i32, status.0) };
        self.diagnostics.dispatch_error(status);
    }
}

#[cfg(target_arch = "arm")]
pub struct NativeGpio;
#[cfg(target_arch = "arm")]
impl GpioHardware for NativeGpio {
    fn write_port(&mut self, port: GpioPort, register: GpioRegister, value: u32) {
        unsafe { core::ptr::write_volatile((0xfff89000 + u32::from(port.index()) * 0x40 + register.offset()) as *mut u32, value) };
    }
    fn save_irq(&mut self) -> SavedIrq { SavedIrq(unsafe { xm1_power_save_irq() }) }
    fn clear_global_enable(&mut self) {
        let register = 0xfff88084 as *mut u32;
        unsafe { core::ptr::write_volatile(register, core::ptr::read_volatile(register) & !1) };
    }
    fn restore_irq(&mut self, saved: SavedIrq) { unsafe { xm1_power_restore_irq(saved.0) }; }
}

#[cfg(target_arch = "arm")]
extern "C" {
    fn xm1_rtos_objects_family_flag_delete(id: i32) -> i32;
    fn xm1_rtos_task_delete_body(id: u32, mode: u32) -> i32;
    fn xm1_s3a_exit_task() -> !;
}
/// # Safety
/// The calling task is `task`.
#[cfg(target_arch = "arm")]
pub unsafe fn retire_current_task(task: crate::shutdown::TaskId) -> ! {
    let slot = u32::from(task.0);
    unsafe {
        let id = (slot | core::ptr::read_volatile((0x7cad9c + (slot - 1) * 4) as *const u32) << 16) as i32;
        let _ = xm1_rtos_objects_family_flag_delete(id);
        let _ = xm1_power_save_irq();
        let _ = xm1_rtos_task_delete_body(id as u32, 0);
        xm1_s3a_exit_task()
    }
}

#[cfg(test)]
#[path = "primitives_tests.rs"]
mod tests;

#[cfg(target_arch = "arm")]
#[no_mangle]
pub extern "C" fn xm1_power_gpio_reset() { reset_gpio(&mut NativeGpio); }
