use crate::updater_serial::{Baud, Error, Handle, Io, PhysicalAddress, UpdaterSerial};
use crate::upload_runtime::{Clock, Delay};
mod sealed { pub trait Port {} }
pub trait Port: sealed::Port {
    fn open(&mut self) -> Result<Handle, Error>;
    fn start(&mut self, handle: Handle) -> Result<(), Error>;
    fn stop(&mut self, handle: Handle) -> Result<(), Error>;
    fn close(&mut self, handle: Handle) -> Result<(), Error>;
    fn baud(&mut self, handle: Handle, baud: Baud) -> Result<(), Error>;
    fn clear_rx(&mut self, handle: Handle, fill: u8) -> Result<(), Error>;
    fn rx_buffer(&mut self, handle: Handle) -> Result<[u8; 100], Error>;
    fn read_position(&mut self, handle: Handle) -> Result<usize, Error>;
    async fn write(&mut self, handle: Handle, bytes: &[u8]) -> Result<(), Error>;
    fn write_register(&self, address: PhysicalAddress, value: u32);
}
impl<I: Io> sealed::Port for UpdaterSerial<I> {}
impl<I: Io> Port for UpdaterSerial<I> {
    fn open(&mut self) -> Result<Handle, Error> { UpdaterSerial::open(self) }
    fn start(&mut self, handle: Handle) -> Result<(), Error> { UpdaterSerial::start(self, handle).map(|_| ()) }
    fn stop(&mut self, handle: Handle) -> Result<(), Error> { UpdaterSerial::stop(self, handle) }
    fn close(&mut self, handle: Handle) -> Result<(), Error> { UpdaterSerial::close(self, handle) }
    fn baud(&mut self, handle: Handle, baud: Baud) -> Result<(), Error> { UpdaterSerial::baud(self, handle, baud) }
    fn clear_rx(&mut self, handle: Handle, fill: u8) -> Result<(), Error> { UpdaterSerial::clear_rx(self, handle, fill) }
    fn rx_buffer(&mut self, handle: Handle) -> Result<[u8; 100], Error> { self.service_receive()?; UpdaterSerial::rx_buffer(self, handle).copied() }
    fn read_position(&mut self, handle: Handle) -> Result<usize, Error> { UpdaterSerial::read_position(self, handle).map(|position| position.get()) }
    async fn write(&mut self, handle: Handle, bytes: &[u8]) -> Result<(), Error> {
        let mut write = self.write_begin(handle, bytes)?;
        core::future::poll_fn(|cx| {
            let result = self.write_poll(&mut write);
            if result.is_pending() { cx.waker().wake_by_ref(); }
            result
        }).await
    }
    fn write_register(&self, address: PhysicalAddress, value: u32) { self.io().write32(address, value); }
}
#[cfg(target_arch = "arm")]
impl sealed::Port for crate::updater_serial::native::NativePort {}
#[cfg(target_arch = "arm")]
impl Port for crate::updater_serial::native::NativePort {
    fn open(&mut self) -> Result<Handle, Error> { Self::open(self) }
    fn start(&mut self, handle: Handle) -> Result<(), Error> { Self::start(self, handle).map(|_| ()) }
    fn stop(&mut self, handle: Handle) -> Result<(), Error> { Self::stop(self, handle) }
    fn close(&mut self, handle: Handle) -> Result<(), Error> { Self::close(self, handle) }
    fn baud(&mut self, handle: Handle, baud: Baud) -> Result<(), Error> { Self::baud(self, handle, baud) }
    fn clear_rx(&mut self, handle: Handle, fill: u8) -> Result<(), Error> { Self::clear_rx(self, handle, fill) }
    fn rx_buffer(&mut self, handle: Handle) -> Result<[u8; 100], Error> { Self::rx_buffer(self, handle) }
    fn read_position(&mut self, handle: Handle) -> Result<usize, Error> { Self::read_position(self, handle).map(|position| position.get()) }
    async fn write(&mut self, handle: Handle, bytes: &[u8]) -> Result<(), Error> {
        let mut write = self.write_begin(handle, bytes)?;
        core::future::poll_fn(|cx| {
            let result = self.write_poll(&mut write);
            if result.is_pending() { cx.waker().wake_by_ref(); }
            result
        }).await
    }
    fn write_register(&self, address: PhysicalAddress, value: u32) { unsafe { core::ptr::write_volatile(address.0 as *mut u32, value); } }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramError { Driver(Error), Response(i32), ImageRange }
impl From<Error> for ProgramError { fn from(error: Error) -> Self { Self::Driver(error) } }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Programmed(());

pub struct SerialProgramState { packet: [u8; 300] }
impl SerialProgramState {
    pub const fn new() -> Self { Self { packet: [0; 300] } }
}

async fn delay(clock: &impl Clock, ticks: u32) { Delay::new(clock, ticks).await; }
fn write<P: Port>(serial: &P, address: u32, value: u32) { serial.write_register(PhysicalAddress(address), value); }

fn checksum(bytes: &[u8]) -> u8 { bytes.iter().fold(0u8, |sum, value| sum.wrapping_add(*value)).wrapping_neg() }
fn response(bytes: &[u8; 100], packet: &[u8; 300], read_position: usize, expected: u8, checked: bool) -> Result<(), ProgramError> {
    if bytes[0] != expected { return Err(ProgramError::Response(-51)); }
    if checked {
        let read_pointer = (0x014e9b08u32 + read_position as u32).to_le_bytes();
        let mut sum = 0u8;
        for index in 0..usize::from(bytes[1]) + 3 {
            let byte = if index < 100 { bytes[index] }
                else if index < 104 { read_pointer[index - 100] }
                else { packet[index - 104] };
            sum = sum.wrapping_add(byte);
        }
        if sum != 0 { return Err(ProgramError::Response(-255)); }
    }
    Ok(())
}
async fn poll<P: Port>(serial: &mut P, state: &SerialProgramState, handle: Handle, expected: u8, checked: bool, attempts: u16, ticks: u32, clock: &impl Clock) -> Result<(), ProgramError> {
    let mut last = Err(ProgramError::Response(-51));
    for _ in 0..attempts {
        delay(clock, ticks).await;
        let read_position = if checked { serial.read_position(handle)? } else { 0 };
        last = response(&serial.rx_buffer(handle)?, &state.packet, read_position, expected, checked);
        if last.is_ok() { return last; }
    }
    last
}
async fn exchange<P: Port>(serial: &mut P, state: &SerialProgramState, handle: Handle, bytes: &[u8], expected: u8, checked: bool, clock: &impl Clock) -> Result<(), ProgramError> {
    serial.clear_rx(handle, 0)?;
    serial.write(handle, bytes).await?;
    poll(serial, state, handle, expected, checked, 50, 10, clock).await
}
async fn packet<P: Port>(serial: &mut P, state: &SerialProgramState, handle: Handle, bytes: &[u8], expected: u8, clock: &impl Clock) -> Result<(), ProgramError> {
    let mut packet = [0; 20];
    let payload = packet.get_mut(..bytes.len()).ok_or(ProgramError::ImageRange)?;
    payload.copy_from_slice(bytes);
    packet[bytes.len()] = checksum(bytes);
    exchange(serial, state, handle, &packet[..bytes.len() + 1], expected, false, clock).await
}
fn word(image: &[u8], offset: usize) -> Result<u32, ProgramError> {
    Ok(u32::from_le_bytes(image.get(offset..offset + 4).ok_or(ProgramError::ImageRange)?.try_into().unwrap()))
}
async fn program_blocks<P: Port>(serial: &mut P, state: &mut SerialProgramState, handle: Handle, data: &[u8], mut address: u32, clock: &impl Clock) -> Result<(), ProgramError> {
    if data.is_empty() { return Err(ProgramError::Response(-255)); }
    for block in data.chunks(256) {
        state.packet.fill(0xff);
        state.packet[0] = 0x50;
        state.packet[1..5].copy_from_slice(&address.to_be_bytes());
        state.packet[5..5 + block.len()].copy_from_slice(block);
        state.packet[0x105] = checksum(&state.packet[..0x105]);
        serial.clear_rx(handle, 0)?;
        serial.write(handle, &state.packet[..0x106]).await?;
        poll(serial, state, handle, 6, false, 50, 10, clock).await?;
        address = address.wrapping_add(0x100);
    }
    Ok(())
}
async fn transfer<P: Port>(serial: &mut P, state: &mut SerialProgramState, handle: Handle, image: &mut [u8], clock: &impl Clock) -> Result<(), ProgramError> {
    serial.clear_rx(handle, 0xff)?;
    serial.write(handle, &[0; 30]).await?;
    poll(serial, state, handle, 0, false, 50, 10, clock).await?;
    exchange(serial, state, handle, &[0x55], 0xe6, false, clock).await?;
    exchange(serial, state, handle, &[0x20], 0x30, true, clock).await?;
    let mut challenge = [0; 4];
    challenge.copy_from_slice(&serial.rx_buffer(handle)?[4..8]);
    let mut answer = [0x10, 4, 0, 0, 0, 0];
    answer[2..].copy_from_slice(&challenge);
    packet(serial, state, handle, &answer, 6, clock).await?;
    exchange(serial, state, handle, &[0x21], 0x31, true, clock).await?;
    packet(serial, state, handle, &[0x11, 1, 0], 6, clock).await?;
    packet(serial, state, handle, &[0x3f, 7, 4, 0x80, 0x0c, 0x80, 2, 1, 1], 6, clock).await?;
    delay(clock, 100).await;
    serial.baud(handle, Baud::new(115_200)?)?;
    exchange(serial, state, handle, &[6], 6, false, clock).await?;
    serial.clear_rx(handle, 0)?;
    serial.write(handle, &[0x40]).await?;
    let mut erased = false;
    let mut ready_to_erase = false;
    for _ in 0..50 {
        delay(clock, 100).await;
        let reply = serial.rx_buffer(handle)?;
        if response(&reply, &state.packet, 0, 0x26, false).is_ok() { erased = true; break; }
        if response(&reply, &state.packet, 0, 0x16, false).is_ok() { ready_to_erase = true; break; }
    }
    if !erased {
        if !ready_to_erase { return Err(ProgramError::Response(-51)); }
        let mut unlock = [0; 19];
        unlock[..3].copy_from_slice(&[0x60, 0x10, 0x52]);
        unlock[3..18].copy_from_slice(b"FxUfJ IlFeInLsM");
        unlock[18] = checksum(&unlock[..18]);
        exchange(serial, state, handle, &unlock, 0x26, false, clock).await?;
        exchange(serial, state, handle, &[0x48], 6, false, clock).await?;
        let mut last = Ok(());
        for sector in 0..30 {
            let mut command = [0x58, 1, sector, 0];
            command[3] = checksum(&command[..3]);
            serial.clear_rx(handle, 0)?;
            serial.write(handle, &command).await?;
            last = poll(serial, state, handle, 6, false, 20, 50, clock).await;
        }
        last?;
        packet(serial, state, handle, &[0x58, 1, 0xff], 6, clock).await?;
    }
    exchange(serial, state, handle, &[0x43], 6, false, clock).await?;
    let first_address = word(image, 0x1c)?;
    let first_bytes = word(image, 0x20)? as usize;
    let first = image.get(0x200..0x200usize.checked_add(first_bytes).ok_or(ProgramError::ImageRange)?).ok_or(ProgramError::ImageRange)?;
    program_blocks(serial, state, handle, first, first_address, clock).await?;
    let second_address = word(image, 0x54)?;
    let second_bytes = word(image, 0x58)? as usize;
    let start = second_address.wrapping_sub(first_address).wrapping_add(0x200) as usize;
    let signature = start.checked_add(0x7a0).ok_or(ProgramError::ImageRange)?;
    image.get_mut(signature..signature + 16).ok_or(ProgramError::ImageRange)?.copy_from_slice(b"UxFRI JfIeFlMsLn");
    let second = image.get(start..start.checked_add(second_bytes).ok_or(ProgramError::ImageRange)?).ok_or(ProgramError::ImageRange)?;
    program_blocks(serial, state, handle, second, second_address, clock).await?;
    state.packet.fill(0xff);
    state.packet[0] = 0x50;
    state.packet[5] = checksum(&state.packet[..5]);
    serial.clear_rx(handle, 0xff)?;
    serial.write(handle, &state.packet[..6]).await?;
    poll(serial, state, handle, 6, false, 50, 10, clock).await
}

fn restore_pins<P: Port>(serial: &P) {
    for (address, value) in [
        (0xfff89304, 0x20000), (0xfff89304, 1), (0xfff89308, 0x20000), (0xfff89308, 15),
        (0xfff8930c, 0x140000), (0xfff8930c, 0), (0xfff89080, 0x80000000), (0xfff89080, 0x40000000),
    ] { write(serial, address, value); }
}
pub struct PowerLease<'a, P: Port>(&'a mut P);
impl<'a, P: Port> PowerLease<'a, P> {
    pub fn new(port: &'a mut P) -> Self { Self(port) }
    pub fn port(&mut self) -> &mut P { self.0 }
}
impl<P: Port> Drop for PowerLease<'_, P> {
    fn drop(&mut self) { write(self.0, 0xfff89084, 0x10000000); }
}
struct Lease<'a, P: Port> { serial: &'a mut P, handle: Option<Handle>, armed: bool }
impl<P: Port> Drop for Lease<'_, P> {
    fn drop(&mut self) {
        if !self.armed { return; }
        if let Some(handle) = self.handle.take() {
            let _ = self.serial.stop(handle);
            let _ = self.serial.close(handle);
        }
        restore_pins(self.serial);
        write(self.serial, 0xfff89084, 0x10000000);
    }
}
pub async fn run<P: Port>(serial: &mut P, state: &mut SerialProgramState, image: &mut [u8], clock: &impl Clock) -> Result<Programmed, ProgramError> {
    let mut lease = Lease { serial, handle: None, armed: true };
    let result = run_inner(lease.serial, state, image, clock, &mut lease.handle).await;
    lease.armed = false;
    result
}

async fn run_inner<P: Port>(serial: &mut P, state: &mut SerialProgramState, image: &mut [u8], clock: &impl Clock, active: &mut Option<Handle>) -> Result<Programmed, ProgramError> {
    for (address, value) in [
        (0xfff89090, 0x3000000), (0xfff89084, 0x10001000),
        (0xfff89080, 0x10000000), (0xfff89080, 0x40004000),
    ] { write(serial, address, value); }
    delay(clock, 20).await;
    write(serial, 0xfff89080, 0x80008000);
    delay(clock, 1000).await;
    let handle = match serial.open() {
        Ok(handle) => handle,
        Err(error) => {
            restore_pins(serial);
            write(serial, 0xfff89084, 0x10000000);
            return Err(ProgramError::Driver(error));
        }
    };
    *active = Some(handle);
    let result = match serial.start(handle) {
        Ok(_) => {
            let result = transfer(serial, state, handle, image, clock).await;
            let _ = serial.stop(handle);
            result
        }
        Err(error) => Err(ProgramError::Driver(error)),
    };
    let _ = serial.close(handle);
    *active = None;
    restore_pins(serial);
    if let Err(error) = result {
        write(serial, 0xfff89084, 0x10000000);
        return Err(error);
    }
    delay(clock, 1000).await;
    write(serial, 0xfff89080, 0x10001000);
    write(serial, 0xfff89080, 0x40004000);
    delay(clock, 20).await;
    write(serial, 0xfff89080, 0x80008000);
    for (address, value) in [
        (0xfff89350, 0x30001), (0xfff89344, 0x1000100), (0xfff8934c, 0x300010),
        (0xfff89344, 0x40004), (0xfff8934c, 0xc000400), (0xfff89344, 0x200000),
    ] { write(serial, address, value); }
    Ok(Programmed(()))
}

