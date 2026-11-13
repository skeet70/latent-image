//! Every guest address the kernel crate uses is here.

use crate::{Error, Form, IrqState, SemaphoreId, Status, TaskId, Timeout};
#[cfg(target_arch = "arm")]
use crate::{Backend, Kernel};

/// The stock kernel's interface root: one word per core, each the core's slot block.
pub(crate) const ROOT_TABLE: u32 = 0x96bd20;
pub(crate) const CORE_SLOT: u32 = 0x1618;
pub(crate) const TASK_SLOT: u32 = 0x16c;
pub(crate) const HANDLER_SLOT: u32 = 0x148;
pub(crate) const IRQ_MASK_SLOT: u32 = 0x164;
pub(crate) const IRQ_RESTORE_SLOT: u32 = 0x168;
pub(crate) const WAIT_SLOT: u32 = 0x7c;
pub(crate) const SIGNAL_SLOT: u32 = 0x6c;
pub(crate) const HANDLER_SIGNAL_SLOT: u32 = 0x70;
pub(crate) const SEMAPHORE_CLASSES: u32 = 0x93e6f4;
pub(crate) const SEMAPHORE_CLASS_LIMIT: u32 = 169;
pub(crate) const ERROR_STATUS: u32 = 0x40a0f82c;
pub(crate) const ERROR_CALLBACK: u32 = 0x4089ea8c;
/// The image's critical section, which masks IRQ, takes the shared word and returns the prior state.
pub(crate) const CRITICAL_ENTER: u32 = 0x7235d8;
/// Its release, which counts down, releases the word and restores the prior state.
pub(crate) const CRITICAL_EXIT: u32 = 0x7236b0;

pub(crate) trait Guest {
    /// # Safety
    /// `address` must name a mapped, aligned word.
    unsafe fn read32(&self, address: u32) -> u32;
    /// # Safety
    /// `address` must name a mapped, aligned word.
    unsafe fn write32(&self, address: u32, value: u32);
    /// # Safety
    /// `address` must name an A32 code word that returns to its caller.
    unsafe fn enter(&self, address: u32, arguments: [u32; 3]) -> u32;
    /// # Safety
    /// The console store must be mapped and the caller must hold the critical section.
    unsafe fn console(&self, kind: u32, semaphore: u32, status: i32);
    /// # Safety
    /// The stock critical section must be mapped.
    unsafe fn critical_enter(&self) -> u32;
    /// # Safety
    /// `saved` must be a word `critical_enter` returned.
    unsafe fn critical_exit(&self, saved: u32);
}

pub struct SlotTable;

pub(crate) fn core_index<G: Guest>(guest: &G) -> u32 {
    unsafe {
        let block = guest.read32(ROOT_TABLE);
        guest.enter(guest.read32(block + CORE_SLOT), [0, 0, 0])
    }
}

pub(crate) fn block<G: Guest>(guest: &G) -> u32 {
    let core = core_index(guest);
    unsafe { guest.read32(ROOT_TABLE + 4 * core) }
}

pub(crate) fn current_task<G: Guest>(guest: &G) -> Option<TaskId> {
    let block = block(guest);
    let word = unsafe { guest.enter(guest.read32(block + TASK_SLOT), [0, 0, 0]) };
    u8::try_from(word).ok().and_then(TaskId::new)
}

pub(crate) fn in_handler<G: Guest>(guest: &G) -> bool {
    let block = block(guest);
    unsafe { guest.enter(guest.read32(block + HANDLER_SLOT), [0, 0, 0]) == 1 }
}

pub(crate) fn irq_mask<G: Guest>(guest: &G) -> IrqState {
    let block = block(guest);
    IrqState::new(unsafe { guest.enter(guest.read32(block + IRQ_MASK_SLOT), [0, 0, 0]) })
}

pub(crate) fn irq_restore<G: Guest>(guest: &G, saved: IrqState) {
    let block = block(guest);
    unsafe { guest.enter(guest.read32(block + IRQ_RESTORE_SLOT), [saved.word(), 0, 0]) };
}

pub(crate) fn cores_lock<G: Guest>(guest: &G) -> IrqState {
    IrqState::new(unsafe { guest.critical_enter() })
}

pub(crate) fn cores_unlock<G: Guest>(guest: &G, saved: IrqState) {
    unsafe { guest.critical_exit(saved.word()) };
}

fn id_word<G: Guest>(guest: &G, id: SemaphoreId) -> u32 {
    let number = u32::from(id.get());
    let class = if number <= SEMAPHORE_CLASS_LIMIT {
        unsafe { guest.read32(SEMAPHORE_CLASSES + 4 * (number - 1)) }
    } else {
        1
    };
    (class << 16) | number
}

/// The report stock's wrappers make for a status other than success: the console record, the status
/// word, and the installed error callback.
unsafe fn report<G: Guest>(guest: &G, kind: u32, id: SemaphoreId, status: Status) {
    unsafe {
        guest.console(kind, u32::from(id.get()), status.0);
        guest.write32(ERROR_STATUS, status.0 as u32);
        let callback = guest.read32(ERROR_CALLBACK);
        guest.enter(callback, [status.0 as u32, 0, 0]);
    }
}

pub(crate) fn semaphore_wait<G: Guest>(guest: &G, id: SemaphoreId, timeout: Timeout) -> Result<(), Error> {
    let block = block(guest);
    let word = unsafe {
        guest.enter(guest.read32(block + WAIT_SLOT), [id_word(guest, id), timeout.word(), 0])
    };
    let status = Status(word as i32);
    match status.check() {
        Ok(()) => Ok(()),
        Err(Error::TimedOut) => Err(Error::TimedOut),
        Err(error) => {
            unsafe { report(guest, 24, id, status) };
            Err(error)
        }
    }
}

pub(crate) fn semaphore_signal<G: Guest>(guest: &G, id: SemaphoreId, form: Form) -> Result<(), Error> {
    let block = block(guest);
    let slot = match form {
        Form::Task => SIGNAL_SLOT,
        Form::Handler => HANDLER_SIGNAL_SLOT,
    };
    let word = unsafe { guest.enter(guest.read32(block + slot), [id_word(guest, id), 0, 0]) };
    let status = Status(word as i32);
    match status.check() {
        Ok(()) => Ok(()),
        Err(error) => {
            unsafe { report(guest, 25, id, status) };
            Err(error)
        }
    }
}

#[cfg(target_arch = "arm")]
pub(crate) struct StockGuest;

#[cfg(target_arch = "arm")]
impl Guest for StockGuest {
    unsafe fn read32(&self, address: u32) -> u32 {
        unsafe { core::ptr::read_volatile(address as *const u32) }
    }
    unsafe fn write32(&self, address: u32, value: u32) {
        unsafe { core::ptr::write_volatile(address as *mut u32, value) }
    }
    unsafe fn enter(&self, address: u32, arguments: [u32; 3]) -> u32 {
        unsafe { xm1_kernel_enter_slot(address, arguments[0], arguments[1], arguments[2]) }
    }
    unsafe fn console(&self, kind: u32, semaphore: u32, status: i32) {
        unsafe { xm1_kernel_console9(kind, semaphore, status) }
    }
    unsafe fn critical_enter(&self) -> u32 {
        unsafe { xm1_kernel_critical_enter() }
    }
    unsafe fn critical_exit(&self, saved: u32) {
        unsafe { xm1_kernel_critical_exit(saved) }
    }
}

#[cfg(target_arch = "arm")]
extern "C" {
    fn xm1_kernel_console9(kind: u32, semaphore: u32, status: i32);
    fn xm1_kernel_critical_enter() -> u32;
    fn xm1_kernel_critical_exit(saved: u32);
}

/// Enters a slot block's stock function word.
#[cfg(target_arch = "arm")]
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn xm1_kernel_enter_slot(address: u32, first: u32, second: u32, third: u32) -> u32 {
    let entry: unsafe extern "C" fn(u32, u32, u32) -> u32 = unsafe { core::mem::transmute(address as usize) };
    unsafe { entry(first, second, third) }
}

impl crate::sealed::Sealed for SlotTable {}

#[cfg(target_arch = "arm")]
impl Backend for SlotTable {
    fn current_task(&self) -> Option<TaskId> { current_task(&StockGuest) }
    fn in_handler(&self) -> bool { in_handler(&StockGuest) }
    fn irq_mask(&self) -> IrqState { irq_mask(&StockGuest) }
    fn irq_restore(&self, saved: IrqState) { irq_restore(&StockGuest, saved) }
    fn cores_lock(&self) -> IrqState { cores_lock(&StockGuest) }
    fn cores_unlock(&self, saved: IrqState) { cores_unlock(&StockGuest, saved) }
    fn semaphore_wait(&self, id: SemaphoreId, timeout: Timeout) -> Result<(), Error> {
        semaphore_wait(&StockGuest, id, timeout)
    }
    fn semaphore_signal(&self, id: SemaphoreId, form: Form) -> Result<(), Error> {
        semaphore_signal(&StockGuest, id, form)
    }
}

#[cfg(target_arch = "arm")]
impl Kernel<SlotTable> {
    /// # Safety
    /// The words at 0x96bd20 and 0x96bd24 hold the two kernels' root blocks.
    pub const unsafe fn stock() -> Self { Kernel::new(SlotTable) }
}

/// The absolute windows this backend reads. The class window starts one entry low because the
/// class read folds `SEMAPHORE_CLASSES + 4 * (id - 1)` to that base.
#[cfg(target_arch = "arm")]
pub(crate) const WINDOWS: [(u32, u32); 4] = [
    (ROOT_TABLE, ROOT_TABLE + 8),
    (SEMAPHORE_CLASSES - 4, SEMAPHORE_CLASSES + 4 * (SEMAPHORE_CLASS_LIMIT + 1)),
    (ERROR_STATUS, ERROR_STATUS + 4),
    (ERROR_CALLBACK, ERROR_CALLBACK + 4),
];

/// The declaration the linked image's admission reads.
#[cfg(target_arch = "arm")]
#[repr(C)]
pub struct Boundary {
    pub magic: u32,
    pub window_count: u32,
    pub entry_count: u32,
    pub windows: [(u32, u32); 4],
    pub entries: [*const (); 12],
}

#[cfg(target_arch = "arm")]
unsafe impl Sync for Boundary {}

#[cfg(target_arch = "arm")]
#[no_mangle]
#[used]
pub static XM1_KERNEL_BOUNDARY: Boundary = Boundary {
    magic: 0x314e524b,
    window_count: 4,
    entry_count: 12,
    windows: WINDOWS,
    entries: [
        core_index::<StockGuest> as *const (),
        block::<StockGuest> as *const (),
        report::<StockGuest> as *const (),
        <SlotTable as Backend>::current_task as *const (),
        <SlotTable as Backend>::in_handler as *const (),
        <SlotTable as Backend>::irq_mask as *const (),
        <SlotTable as Backend>::irq_restore as *const (),
        <SlotTable as Backend>::cores_lock as *const (),
        <SlotTable as Backend>::cores_unlock as *const (),
        <SlotTable as Backend>::semaphore_wait as *const (),
        <SlotTable as Backend>::semaphore_signal as *const (),
        xm1_kernel_enter_slot as *const (),
    ],
};
