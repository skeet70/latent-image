use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::Poll;
use super::{Baud, Error, Handle, InterruptBinding, Io, NativeIo, PhysicalAddress, ReceiveErrors, RxEpoch, State, UpdaterSerial, Write, RX_CAPACITY};

struct Storage(UnsafeCell<MaybeUninit<UpdaterSerial<NativeIo>>>);
// Access is confined to one core with IRQ and FIQ masked.
unsafe impl Sync for Storage {}
static STORAGE: Storage = Storage(UnsafeCell::new(MaybeUninit::uninit()));
static CLAIMED: AtomicBool = AtomicBool::new(false);
static INITIALIZED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindError { AlreadyOwned, InvalidBinding }
pub struct NativePort { _core: core::marker::PhantomData<*mut ()> }
impl NativePort {
    /// # Safety
    /// The calling core exclusively owns UART0, GPIO C0/C1, clock bit zero, the
    /// writable IRQ table and IRQ107. All calls and interrupt dispatch stay on this core.
    pub unsafe fn claim(table: PhysicalAddress, default_handler: PhysicalAddress, controller: PhysicalAddress) -> Result<Self, BindError> {
        let io = NativeIo { _private: () };
        let saved = io.mask_interrupts();
        let binding = unsafe { InterruptBinding::new(table, PhysicalAddress(native_receive_irq as *const () as u32), default_handler, controller) };
        let Some(binding) = binding else { io.restore_interrupts(saved); return Err(BindError::InvalidBinding); };
        if CLAIMED.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            io.restore_interrupts(saved);
            return Err(BindError::AlreadyOwned);
        }
        let mut serial = UpdaterSerial::new(io, binding);
        if INITIALIZED.load(Ordering::Acquire) {
            let previous = unsafe { (*STORAGE.0.get()).assume_init_ref() };
            serial.generation = previous.generation;
            serial.epoch = previous.epoch;
            serial.write_sequence = previous.write_sequence;
            serial.wire_pending = previous.wire_pending;
        }
        unsafe { (*STORAGE.0.get()).write(serial); }
        INITIALIZED.store(true, Ordering::Release);
        (NativeIo { _private: () }).restore_interrupts(saved);
        Ok(Self { _core: core::marker::PhantomData })
    }
    fn access<R>(&mut self, operation: impl FnOnce(&mut UpdaterSerial<NativeIo>) -> R) -> R {
        let io = NativeIo { _private: () };
        let saved = io.mask_interrupts();
        let serial = unsafe { (*STORAGE.0.get()).assume_init_mut() };
        let result = operation(serial);
        io.restore_interrupts(saved);
        result
    }
    pub fn open(&mut self) -> Result<Handle, Error> { self.access(UpdaterSerial::open) }
    pub fn start(&mut self, handle: Handle) -> Result<RxEpoch, Error> { self.access(|serial| serial.start(handle)) }
    pub fn stop(&mut self, handle: Handle) -> Result<(), Error> { self.access(|serial| serial.stop(handle)) }
    pub fn close(&mut self, handle: Handle) -> Result<(), Error> { self.access(|serial| serial.close(handle)) }
    pub fn baud(&mut self, handle: Handle, baud: Baud) -> Result<(), Error> { self.access(|serial| serial.baud(handle, baud)) }
    pub fn reset_rx(&mut self, handle: Handle) -> Result<(), Error> { self.access(|serial| serial.reset_rx(handle)) }
    pub fn clear_rx(&mut self, handle: Handle, fill: u8) -> Result<(), Error> { self.access(|serial| serial.clear_rx(handle, fill)) }
    pub fn rx_buffer(&mut self, handle: Handle) -> Result<[u8; RX_CAPACITY], Error> { self.access(|serial| serial.rx_buffer(handle).copied()) }
    pub fn read_position(&mut self, handle: Handle) -> Result<super::ReadPosition, Error> { self.access(|serial| serial.read_position(handle)) }
    pub fn read_received(&mut self, handle: Handle, output: &mut [u8]) -> Result<usize, Error> { self.access(|serial| serial.read_received(handle, output)) }
    pub fn receive_errors(&mut self) -> ReceiveErrors { self.access(|serial| serial.receive_errors()) }
    pub fn write_begin<'a>(&mut self, handle: Handle, bytes: &'a [u8]) -> Result<Write<'a>, Error> { self.access(|serial| serial.write_begin(handle, bytes)) }
    pub fn write_poll(&mut self, write: &mut Write<'_>) -> Poll<Result<(), Error>> { self.access(|serial| serial.write_poll(write)) }
    pub fn cancel_write(&mut self, write: &Write<'_>) -> Result<(), Error> { self.access(|serial| serial.cancel_write(write)) }
    pub fn write(&mut self, handle: Handle, bytes: &[u8]) -> Result<(), Error> {
        let mut write = self.write_begin(handle, bytes)?;
        loop {
            match self.write_poll(&mut write) {
                Poll::Ready(result) => return result,
                Poll::Pending => core::hint::spin_loop(),
            }
        }
    }
}
impl Drop for NativePort {
    fn drop(&mut self) {
        let io = NativeIo { _private: () };
        let saved = io.mask_interrupts();
        let serial = unsafe { (*STORAGE.0.get()).assume_init_mut() };
        if serial.state != State::Closed { let _ = serial.close(Handle(serial.generation)); }
        CLAIMED.store(false, Ordering::Release);
        io.restore_interrupts(saved);
    }
}
extern "C" fn native_receive_irq() {
    let io = NativeIo { _private: () };
    let saved = io.mask_interrupts();
    if CLAIMED.load(Ordering::Acquire) {
        let serial = unsafe { (*STORAGE.0.get()).assume_init_mut() };
        if serial.state == State::Receiving {
            let _ = serial.receive_interrupt(RxEpoch(serial.epoch));
        }
    }
    io.restore_interrupts(saved);
}
