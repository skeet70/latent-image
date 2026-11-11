use super::*;
use core::ptr::{read_volatile, write_volatile};
core::arch::global_asm!(include_str!("serial_write_residue.S"));
extern "C" {
    fn xm1_power_semaphore_wait(id: i32, timeout: i32) -> i32;
    fn xm1_power_semaphore_signal(id: i32) -> i32;
    fn xm1_power_delay_us(delay: u32);
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_save_irq() -> u32;
    fn xm1_power_restore_irq(saved: u32);
    fn xm1_power_timer_release_handle(handle: u32) -> i32;
}
struct PhysicalSerial;
impl Hardware for PhysicalSerial {
    fn read(&mut self, channel: Channel, register: Register) -> u32 { unsafe { read_volatile((channel.bank() + register.offset()) as *const u32) } }
    fn write(&mut self, channel: Channel, register: Register, value: u32) { unsafe { write_volatile((channel.bank() + register.offset()) as *mut u32, value) } }
    fn select(&mut self, pin: Pin, active: bool) { unsafe { write_volatile(pin.address() as *mut u32, pin.output(active)) } }
    fn delay(&mut self, delay: Delay) { unsafe { xm1_power_delay_us(delay.0 as u32) } }
    fn update(&mut self, channel: Channel, register: Register, clear: u32, set: u32) {
        unsafe {
            let saved = xm1_power_save_irq();
            let value = self.read(channel, register);
            self.write(channel, register, (value & !clear) | set);
            xm1_power_restore_irq(saved);
        }
    }
}
fn state() -> *mut State { 0x409c2644 as *mut State }
fn code(result: Result<(), Error>) -> i32 { match result { Ok(()) => 0, Err(error) => error as i32 } }

#[no_mangle]
pub unsafe extern "C" fn xm1_power_serial_open(port: u32, mode: u32, out: *mut u32) -> i32 {
    let Some(endpoint) = Endpoint::parse(port) else { return Error::InvalidEndpoint as i32; };
    let channel = endpoint.descriptor().channel;
    if xm1_power_semaphore_wait(channel.semaphore(), -1) != 0 { return Error::Semaphore as i32; }
    let saved = xm1_power_interrupt_lock();
    let result = (*state()).reserve(endpoint, &mut *(0x409c2804 as *mut u32));
    xm1_power_interrupt_unlock(saved);
    match result {
        Ok(handle) => {
            write_volatile(out, handle.raw());
            State::initialize(&mut PhysicalSerial, endpoint.descriptor(), OpenMode::from_abi(mode));
            0
        }
        Err(error) => error as i32,
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_serial_configure(handle: u32, delay: u32) -> i32 {
    code((*state()).configure(&mut PhysicalSerial, Handle::from_abi(handle), Delay(delay as u8)))
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_serial_write_with_residue(handle: u32, word: u32, residue: u32) -> i32 {
    code((*state()).write(&mut PhysicalSerial, Handle::from_abi(handle), SerialWord(word as u16), StatusResidue(residue)))
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_serial_status(handle: u32, out: *mut u32) -> i32 {
    match (*state()).status(&mut PhysicalSerial, Handle::from_abi(handle)) {
        Ok(status) => { write_volatile(out as *mut Status, status); 0 }
        Err(error) => error as i32,
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_serial_read(handle: u32, out: *mut u16) -> i32 {
    match (*state()).read(&mut PhysicalSerial, Handle::from_abi(handle)) {
        Ok(word) => { write_volatile(out, word.0); 0 }
        Err(error) => error as i32,
    }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_serial_stop(handle: u32) -> i32 {
    let handle = Handle::from_abi(handle);
    let channel = match (*state()).resolve(handle) { Ok((_, descriptor)) => descriptor.channel, Err(error) => return error as i32 };
    let status = code((*state()).stop(&mut PhysicalSerial, handle));
    if status == 0 {
        for base in [0x409c2674usize, 0x409c2684] {
            let handle = read_volatile((base + channel.index() * 4) as *const u32);
            if handle != 0 { let _ = dma_stop(DmaHandle(handle)); }
        }
    }
    status
}
#[no_mangle]
pub unsafe extern "C" fn xm1_power_serial_close(handle: u32) -> i32 {
    match (*state()).begin_close(&mut PhysicalSerial, Handle::from_abi(handle)) {
        Ok(channel) => {
            for base in [0x409c2674usize, 0x409c2684] {
                let slot = (base + channel.index() * 4) as *mut u32;
                let handle = read_volatile(slot);
                if handle != 0 { let _ = dma_close(DmaHandle(handle)); write_volatile(slot, 0); }
            }
            (*state()).handles[channel.index()] = 0;
            if xm1_power_semaphore_signal(channel.semaphore()) == 0 { 0 } else { Error::Semaphore as i32 }
        }
        Err(error) => error as i32,
    }
}

#[derive(Clone, Copy)]
struct DmaHandle(u32);
#[derive(Clone, Copy)]
struct DmaChannel(u32);
impl DmaChannel {
    fn state(self) -> *mut u32 { (0x918b98 + self.0 * 4) as *mut u32 }
    fn register(self, offset: u32) -> *mut u32 { (0xfff02000 + self.0 * 64 + offset) as *mut u32 }
    fn interrupt(self) -> *mut u32 { (0x918bf8 + self.0 * 4) as *mut u32 }
}
unsafe fn dma_channel(handle: DmaHandle) -> Option<DmaChannel> {
    if handle.0 == 0 { return None; }
    (0..8).find(|channel| read_volatile((0x918bd8 + channel * 4) as *const u32) == handle.0).map(DmaChannel)
}
unsafe fn dma_release_interrupt(channel: DmaChannel) -> i32 {
    let handle = read_volatile(channel.interrupt());
    if handle == 0 { return 0; }
    let status = xm1_power_timer_release_handle(handle);
    write_volatile(channel.interrupt(), 0);
    status
}
unsafe fn dma_stop(handle: DmaHandle) -> i32 {
    let Some(channel) = dma_channel(handle) else { return -20; };
    if read_volatile(channel.state()) != 2 { return -11; }
    let control = channel.register(0x28);
    write_volatile(control, read_volatile(control) | 0x100);
    loop {
        let mut status = read_volatile(channel.register(0x24));
        if status & 1 == 0 { break; }
        if status & 8 != 0 {
            write_volatile(control, read_volatile(control) | 2);
            while status & 4 != 0 { status = read_volatile(channel.register(0x24)); }
            break;
        }
    }
    write_volatile(control, read_volatile(control) | 8);
    let status = dma_release_interrupt(channel);
    write_volatile(channel.state(), 1);
    status
}
unsafe fn dma_close(handle: DmaHandle) -> i32 {
    let Some(channel) = dma_channel(handle) else { return -20; };
    let status = dma_release_interrupt(channel);
    write_volatile((0x918bb8 + channel.0 * 4) as *mut u32, u32::MAX);
    write_volatile(channel.state(), 0);
    write_volatile((0x918bd8 + channel.0 * 4) as *mut u32, 0);
    if xm1_power_semaphore_signal(channel.0 as i32 + 11) != 0 { -255 } else { status }
}
