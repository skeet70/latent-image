use crate::{driver::{DelayWaiter, QueueDriver, QueuePorts, Service, ServiceTask}, interrupts, DelayMessage, DelayTask, Delays, Error, HandleWord, Milliseconds, Repeat, SlotIndex, TaskPorts, TimerHandle, TimerPorts, Timers};
use core::{cell::{Cell, UnsafeCell}, num::NonZeroU32, ptr::{read_volatile, write_volatile}};
use xm1_kernel::{Backend, Context, Header, IrqState, Kernel, Lockable, PostError, QueueId, SlotTable, StackBytes, TaskArgument, TaskConfig, TaskFlagPolicy, TaskId, TaskPriority};

const TIMER_HANDLES: u32 = 0x95aa9c;
const IRQ_HANDLES: u32 = 0x940e9c;
const IRQ_ROUTES: u32 = 0x7c9124;
const DELAY_SHARED: u32 = 0x95dc08;
const TIMER_SHARED: u32 = 0x96bd40;
const TICKS: u32 = 0x96e97c;
const DELAY_ARGUMENT: u32 = 0x89f76c;
const TIMER_ARGUMENT: u32 = 0x89f770;

/// A retained entry taking up to four word arguments and returning a status word.
#[derive(Clone, Copy)]
struct StockEntry(u32);
const HANDLE_ALLOCATE: StockEntry = StockEntry(0x65722c);
const HANDLE_RESOLVE: StockEntry = StockEntry(0x657384);
const HANDLE_RELEASE: StockEntry = StockEntry(0x65744c);
const DIAGNOSTIC: StockEntry = StockEntry(0x726c74);
const IRQ_CONFIGURE: StockEntry = StockEntry(0x723870);
const IRQ_DETACH: StockEntry = StockEntry(0x7239a0);
const DEVICE_ACQUIRE: StockEntry = StockEntry(0x655b28);
const DEVICE_CONFIGURE: StockEntry = StockEntry(0x655ccc);
const DEVICE_STOP: StockEntry = StockEntry(0x655fe8);
const DEVICE_RELEASE: StockEntry = StockEntry(0x655c24);

/// Class-2 events this unit reports to the retained diagnostic service.
#[derive(Clone, Copy)]
enum Diagnostic { AttachRefused = 15, ServiceDown = 28, ShutdownFailed = 29, NoFreeSlot = 31 }

const KIND_TIMER: u32 = 32;
const KIND_INTERRUPT: u32 = 128;

#[no_mangle]
pub static XM1_REQUIRES_LENS_SAFETY_CONTRACT: u32 = 1;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct StockCode(NonZeroU32);
impl StockCode {
    pub const fn new(word: u32) -> Option<Self> {
        match NonZeroU32::new(word) { Some(word) => Some(Self(word)), None => None }
    }
    unsafe fn invoke(self) {
        let entry: unsafe extern "C" fn() = unsafe { core::mem::transmute(self.0.get() as usize) };
        unsafe { entry() };
    }
}

#[derive(Clone, Copy)]
pub enum Callback { Owned(fn()), Stock(StockCode), DelayTick }
impl Callback {
    fn invoke(self) { match self { Self::Owned(entry) => entry(), Self::Stock(entry) => unsafe { entry.invoke() }, Self::DelayTick => xm1_time_delay_tick() } }
    fn expire(self) {
        match self { Self::Owned(entry) => xm1_time_owned_expiry_callback(entry), Self::Stock(entry) => xm1_time_expiry_callback(entry), Self::DelayTick => xm1_time_delay_tick() }
    }
}

unsafe fn read(address: u32) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn write(address: u32, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }
unsafe fn call(entry: StockEntry, arguments: [u32; 4]) -> i32 {
    let entry: unsafe extern "C" fn(u32, u32, u32, u32) -> i32 = unsafe { core::mem::transmute(entry.0 as usize) };
    unsafe { entry(arguments[0], arguments[1], arguments[2], arguments[3]) }
}
fn checked(status: i32) -> Result<(), Error> { if status == 0 { Ok(()) } else { Err(Error::Backend(status)) } }
fn diagnostic(event: Diagnostic) { unsafe { call(DIAGNOSTIC, [2, event as u32, 0, 0]) }; }
/// Reports a timer start failure as stock's start does before returning it.
fn report(error: Error) -> Error {
    match error {
        Error::Uninitialized => diagnostic(Diagnostic::ServiceDown),
        Error::Exhausted => diagnostic(Diagnostic::NoFreeSlot),
        Error::Backend(_) => {}
    }
    error
}
fn fault(service: Service, post: bool) {
    if let Some(callback) = StockCode::new(unsafe { read(shared(service) + 20) }) {
        match (service, post) {
            (Service::Delay, false) => xm1_time_delay_fault(callback),
            (Service::Delay, true) => xm1_time_delay_post_fault(callback),
            (Service::Timer, _) => xm1_time_timer_post_fault(callback),
        }
    }
}
fn kernel() -> Kernel<SlotTable> { unsafe { Kernel::stock() } }

fn locked<R>(operation: impl FnOnce() -> R) -> R {
    let kernel = kernel();
    match kernel.context() {
        Context::Task(mut cx) => { let _lock = cx.lock_cores(); operation() }
        Context::Interrupt(mut cx) => { let _lock = cx.lock_cores(); operation() }
        Context::Masked(mut cx) => { let _lock = cx.lock_cores(); operation() }
    }
}

struct TickLock { kernel: Kernel<SlotTable>, saved: Option<IrqState> }
impl TickLock {
    fn new() -> Self {
        let kernel = kernel();
        let saved = kernel.backend().cores_lock();
        Self { kernel, saved: Some(saved) }
    }
    fn invoke(&mut self, callback: Callback) {
        if let Some(saved) = self.saved.take() { self.kernel.backend().cores_unlock(saved); }
        callback.expire();
        self.saved = Some(self.kernel.backend().cores_lock());
    }
}
impl Drop for TickLock {
    fn drop(&mut self) {
        if let Some(saved) = self.saved.take() { self.kernel.backend().cores_unlock(saved); }
    }
}

struct Shared<T>(UnsafeCell<T>);
// Access is serialized by the retained reentrant cross-core lock.
unsafe impl<T: Send> Sync for Shared<T> {}
impl<T> Shared<T> {
    const fn new(value: T) -> Self { Self(UnsafeCell::new(value)) }
    fn get(&self) -> &T { unsafe { &*self.0.get() } }
}
static TIMERS: Shared<Timers<Callback>> = Shared::new(Timers::new());
static DELAYS: Shared<Delays> = Shared::new(Delays::new());
static DELAY_TIMER: Shared<Cell<Option<TimerHandle>>> = Shared::new(Cell::new(None));
static DELAY_WAITERS: Shared<[DelayWaiter; 104]> = Shared::new([const { DelayWaiter::new() }; 104]);

struct Handles<'a>(Option<&'a mut TickLock>);
impl TimerPorts<Callback> for Handles<'_> {
    fn allocate(&mut self, slot: SlotIndex) -> Result<TimerHandle, Error> {
        checked(unsafe { call(HANDLE_ALLOCATE, [u32::from(slot.get()), KIND_TIMER, TIMER_HANDLES, 0]) })?;
        TimerHandle::new(unsafe { read(TIMER_HANDLES + 4 * u32::from(slot.get())) }).ok_or(Error::Backend(-30))
    }
    fn resolve(&mut self, timer: HandleWord) -> Result<SlotIndex, Error> {
        let mut index = 0u32;
        checked(unsafe { call(HANDLE_RESOLVE, [timer.0, KIND_TIMER, TIMER_HANDLES, (&mut index as *mut u32) as u32]) })?;
        u8::try_from(index).ok().and_then(SlotIndex::new).ok_or(Error::Backend(-30))
    }
    fn release(&mut self, slot: SlotIndex) { unsafe { call(HANDLE_RELEASE, [u32::from(slot.get()), KIND_TIMER, TIMER_HANDLES, 0]) }; }
    fn invoke(&mut self, callback: Callback) {
        match &mut self.0 { Some(lock) => lock.invoke(callback), None => callback.expire() }
    }
}
struct Interrupts;
impl interrupts::Ports<StockCode> for Interrupts {
    fn route(&mut self, source: interrupts::Source) -> Result<interrupts::Route, interrupts::Error> {
        let base = IRQ_ROUTES + 12 * u32::from(source.get());
        Ok(interrupts::Route {
            line: interrupts::Line::new(unsafe { read(base) }).ok_or(interrupts::Error::BadSource)?,
            priority: interrupts::Priority::new(unsafe { read(base + 4) }).ok_or(interrupts::Error::BadSource)?,
            trigger: interrupts::Trigger::new(unsafe { read(base + 8) }).ok_or(interrupts::Error::BadSource)?,
        })
    }
    fn allocate(&mut self, line: interrupts::Line) -> Result<interrupts::InterruptHandle, interrupts::Error> {
        let result = unsafe { call(HANDLE_ALLOCATE, [u32::from(line.get()), KIND_INTERRUPT, IRQ_HANDLES, 0]) };
        if result != 0 { return Err(interrupts::Error::Allocate(result)); }
        interrupts::InterruptHandle::new(unsafe { read(IRQ_HANDLES + 4 * u32::from(line.get())) }).ok_or(interrupts::Error::InvalidHandle)
    }
    fn configure(&mut self, route: interrupts::Route, handler: StockCode) -> Result<(), interrupts::Error> {
        if unsafe { call(IRQ_CONFIGURE, [u32::from(route.line.get()), u32::from(route.priority.get()), route.trigger.word(), handler.0.get()]) } == 0 { Ok(()) } else { Err(interrupts::Error::Configure) }
    }
    fn publish_source(&mut self, line: interrupts::Line, source: Option<interrupts::Source>) {
        unsafe { write(IRQ_HANDLES + 4 * (KIND_INTERRUPT + u32::from(line.get())), source.map(|source| u32::from(source.get())).unwrap_or(u32::MAX)) };
    }
    fn resolve(&mut self, handle: HandleWord) -> Result<interrupts::Line, interrupts::Error> {
        let mut index = 0u32;
        let result = unsafe { call(HANDLE_RESOLVE, [handle.0, KIND_INTERRUPT, IRQ_HANDLES, (&mut index as *mut u32) as u32]) };
        if result != 0 { return Err(interrupts::Error::InvalidHandle); }
        interrupts::Line::new(index).ok_or(interrupts::Error::InvalidHandle)
    }
    fn detach(&mut self, line: interrupts::Line) -> Result<(), interrupts::Error> {
        if unsafe { call(IRQ_DETACH, [u32::from(line.get()), 0, 0, 0]) } == 0 { Ok(()) } else { Err(interrupts::Error::Configure) }
    }
    fn release(&mut self, line: interrupts::Line) { unsafe { call(HANDLE_RELEASE, [u32::from(line.get()), KIND_INTERRUPT, IRQ_HANDLES, 0]) }; }
}
struct Effects(Service);
impl TaskPorts<Callback> for Effects {
    fn invoke(&mut self, callback: Callback) { callback.invoke(); }
    fn fault(&mut self) { fault(self.0, false); }
}
impl QueuePorts<Callback> for Effects {
    fn received(&mut self, header: Header, payload: &[u8]) {
        let view = shared(self.0);
        let word = u32::from(header.operation) | (u32::from(header.sender) << 16) | (u32::from(header.destination) << 24);
        unsafe {
            write(view, word);
            write(view + 4, payload.len() as u32);
            // The view's word at +8 is the receiver's payload destination.
            if !payload.is_empty() { core::ptr::copy_nonoverlapping(payload.as_ptr(), read(view + 8) as *mut u8, payload.len()); }
        }
    }
}
fn dispatch_delay() { dispatch(Service::Delay); }
fn dispatch_timer() { dispatch(Service::Timer); }
fn dispatch(service: Service) {
    if let Some(callback) = StockCode::new(unsafe { read(shared(service) + 16) }) {
        match service { Service::Delay => xm1_time_delay_callback(callback), Service::Timer => xm1_time_timer_callback(callback) }
    }
}

fn queue(service: Service) -> QueueId { QueueId::new(match service { Service::Delay => 6, Service::Timer => 9 }).unwrap() }
fn queue_driver(service: Service, last: DelayMessage) -> QueueDriver<Callback> { QueueDriver::new(service, queue(service), last) }
fn shared(service: Service) -> u32 { match service { Service::Delay => DELAY_SHARED, Service::Timer => TIMER_SHARED } }
fn config(service: Service) -> TaskConfig {
    let (id, name, entry, argument): (u8, _, extern "C" fn(u32), _) = match service {
        Service::Delay => (6, c"delay", xm1_time_delay_task, DELAY_ARGUMENT),
        Service::Timer => (9, c"timer", xm1_time_timer_task, TIMER_ARGUMENT),
    };
    TaskConfig {
        id: TaskId::new(id).unwrap(), name,
        priority: TaskPriority::new(3).unwrap(), stack: StackBytes::new(3072).unwrap(),
        entry, argument: TaskArgument(argument), flags: TaskFlagPolicy::ClearAfterWait,
    }
}
fn create(service: Service) -> i32 {
    let kernel = kernel();
    let Some(cx) = kernel.task() else { return -255; };
    match ServiceTask::create(&cx, queue(service), config(service)) {
        Ok(_) => 0,
        Err(_) => -255,
    }
}
fn delete(service: Service) -> i32 {
    let kernel = kernel();
    let Some(cx) = kernel.task() else { return -255; };
    let queue = queue(service);
    ServiceTask::stock(queue, TaskId::new(queue.get()).unwrap()).delete(&cx).map(|()| 0).unwrap_or(-255)
}
fn notify(service: Service) {
    let kernel = kernel();
    let result = match kernel.context() {
        Context::Task(cx) => queue_driver(service, DelayMessage::Unsupported).notify(&cx),
        Context::Interrupt(cx) => queue_driver(service, DelayMessage::Unsupported).notify(&cx),
        Context::Masked(_) => Err(PostError::Kernel(xm1_kernel::Error::Context)),
    };
    if result.is_err() { fault(service, true); }
    if service == Service::Timer { unsafe { write(TICKS, read(TICKS).wrapping_add(1)) }; }
}
fn arm(service: Service, device: u32, callback: u32, fault_callback: u32) -> i32 {
    let base = shared(service);
    let result = unsafe { call(DEVICE_ACQUIRE, [device, base + 12, 0, 0]) };
    if result != 0 { return result; }
    let descriptor = match service {
        Service::Delay => [1000, 1, 1, xm1_time_notify_delay as *const () as u32],
        Service::Timer => [2500, 1, 1, xm1_time_notify_timer as *const () as u32],
    };
    let result = unsafe { call(DEVICE_CONFIGURE, [read(base + 12), descriptor.as_ptr() as u32, 0, 0]) };
    if result == 0 {
        unsafe { write(base + 16, callback) };
        if service == Service::Timer { unsafe { write(base + 20, fault_callback); write(TICKS, 0); } }
    }
    result
}
fn disarm(service: Service) -> i32 {
    let handle = unsafe { read(shared(service) + 12) };
    let result = unsafe { call(DEVICE_STOP, [handle, 0, 0, 0]) };
    if result == 0 { unsafe { call(DEVICE_RELEASE, [handle, 0, 0, 0]) } } else { result }
}

#[no_mangle]
pub extern "C" fn xm1_time_initialize(fault_callback: u32) -> i32 {
    if locked(|| TIMERS.get().initialize()).is_err() { return -255; }
    unsafe {
        write(TIMER_HANDLES + 128, 1);
        write(TIMER_ARGUMENT, 2); write(TIMER_ARGUMENT + 4, 7); write(TIMER_ARGUMENT + 8, xm1_time_tick as *const () as u32); write(TIMER_ARGUMENT + 12, fault_callback);
    }
    let result = create(Service::Timer);
    if result != 0 { diagnostic(Diagnostic::ServiceDown); -32 } else { 0 }
}
#[no_mangle]
pub extern "C" fn xm1_time_shutdown() -> i32 {
    let initialized = unsafe { read(TIMER_HANDLES + 128) } == 1;
    locked(|| TIMERS.get().shutdown(&mut Handles(None)));
    let result = if initialized { xm1_time_delete_timer() } else { 0 };
    unsafe { write(TIMER_HANDLES + 128, 0) };
    if result == 0 { 0 } else { diagnostic(Diagnostic::ShutdownFailed); -255 }
}
#[no_mangle]
pub unsafe extern "C" fn xm1_time_start(period: u32, repeat: u32, callback: u32, output: *mut u32) -> i32 {
    locked(|| {
    match TIMERS.get().start(Milliseconds(period), if repeat == 1 { Repeat::Periodic } else { Repeat::Once }, StockCode::new(callback).map(Callback::Stock), &mut Handles(None)) {
        Ok(handle) => { unsafe { write_volatile(output, handle.word()) }; 0 }
        Err(error) => report(error).status(),
    }
    })
}
#[no_mangle]
pub extern "C" fn xm1_time_cancel(handle: u32) -> i32 {
    locked(|| TIMERS.get().cancel(HandleWord(handle), &mut Handles(None)));
    0
}
#[no_mangle]
pub extern "C" fn xm1_time_reload(handle: u32, period: u32) -> i32 {
    locked(|| TIMERS.get().set_reload(HandleWord(handle), Milliseconds(period), &mut Handles(None)).map(|()| 0).unwrap_or_else(Error::status))
}
#[no_mangle]
pub extern "C" fn xm1_time_tick() {
    let mut lock = TickLock::new();
    TIMERS.get().tick(&mut Handles(Some(&mut lock)));
}
#[no_mangle]
pub extern "C" fn xm1_time_ticks() -> u32 { unsafe { read(TICKS) } }
#[no_mangle]
pub unsafe extern "C" fn xm1_time_attach(source: u32, callback: u32, output: *mut u32) -> i32 {
    let (Some(source), Some(callback)) = (interrupts::Source::new(source), StockCode::new(callback)) else {
        diagnostic(Diagnostic::AttachRefused);
        return -30;
    };
    let result = locked(|| {
        let result = interrupts::attach(source, callback, &mut Interrupts);
        if let Ok(handle) = result { unsafe { write_volatile(output, handle.word()) }; }
        result
    });
    match result {
        Ok(_) => 0,
        Err(error) => {
            if matches!(error, interrupts::Error::Allocate(_)) { diagnostic(Diagnostic::AttachRefused); }
            error.status()
        }
    }
}
#[no_mangle]
pub extern "C" fn xm1_time_release(handle: u32) -> i32 {
    interrupts::release(HandleWord(handle), &mut Interrupts).map(|()| 0).unwrap_or_else(interrupts::Error::status)
}
#[no_mangle]
pub extern "C" fn xm1_time_notify_delay() { notify(Service::Delay); }
#[no_mangle]
pub extern "C" fn xm1_time_notify_timer() { notify(Service::Timer); }
#[no_mangle]
pub extern "C" fn xm1_time_create_delay(fault_callback: u32) -> i32 {
    unsafe { write(DELAY_ARGUMENT, fault_callback) }; create(Service::Delay)
}
#[no_mangle]
pub extern "C" fn xm1_time_delete_delay() -> i32 { delete(Service::Delay) }
#[no_mangle]
pub extern "C" fn xm1_time_create_timer(device: u32, argument: u32, callback: u32, fault_callback: u32) -> i32 {
    unsafe { write(TIMER_ARGUMENT, device); write(TIMER_ARGUMENT + 4, argument); write(TIMER_ARGUMENT + 8, callback); write(TIMER_ARGUMENT + 12, fault_callback); }
    create(Service::Timer)
}
#[no_mangle]
pub extern "C" fn xm1_time_delete_timer() -> i32 {
    let result = disarm(Service::Timer); if result == 0 { delete(Service::Timer) } else { result }
}
#[no_mangle]
pub extern "C" fn xm1_time_arm_delay(device: u32, callback: u32) -> i32 { arm(Service::Delay, device, callback, 0) }
#[no_mangle]
pub extern "C" fn xm1_time_arm_timer(device: u32, _: u32, callback: u32, fault_callback: u32) { arm(Service::Timer, device, callback, fault_callback); }
#[no_mangle]
pub extern "C" fn xm1_time_disarm_delay() -> i32 { disarm(Service::Delay) }
// Only create() starts the two service tasks, each with its fixed argument block.
#[no_mangle]
pub extern "C" fn xm1_time_delay_task(_: u32) {
    unsafe { write(DELAY_SHARED + 20, read(DELAY_ARGUMENT)) };
    let kernel = kernel();
    let Some(cx) = kernel.task() else { return; };
    let last = if unsafe { read(DELAY_SHARED) } as u16 == 0x1600 { DelayMessage::Tick } else { DelayMessage::Unsupported };
    let driver = queue_driver(Service::Delay, last);
    driver.set_callback(Some(Callback::Owned(dispatch_delay)));
    loop { driver.receive(&cx, &mut Effects(Service::Delay)); }
}
#[no_mangle]
pub extern "C" fn xm1_time_timer_task(_: u32) {
    unsafe { arm(Service::Timer, read(TIMER_ARGUMENT), read(TIMER_ARGUMENT + 8), read(TIMER_ARGUMENT + 12)) };
    let kernel = kernel();
    let Some(cx) = kernel.task() else { return; };
    let driver = queue_driver(Service::Timer, DelayMessage::Unsupported);
    driver.set_callback(Some(Callback::Owned(dispatch_timer)));
    loop { driver.receive(&cx, &mut Effects(Service::Timer)); }
}

#[no_mangle]
extern "C" fn xm1_time_delay_tick() {
    let signals = locked(|| {
        let mut signals = [None; 104];
        if !DELAYS.get().tick(|task| {
            let index = usize::from(task.get() - 1);
            signals[index] = DELAY_WAITERS.get()[index].semaphore();
        }) {
            if let Some(handle) = DELAY_TIMER.get().take() { TIMERS.get().cancel(handle.into(), &mut Handles(None)); }
        }
        signals
    });
    let kernel = kernel();
    for semaphore in signals.into_iter().flatten() {
        match kernel.context() {
            Context::Task(cx) => { let _ = semaphore.signal(&cx); }
            Context::Interrupt(cx) => { let _ = semaphore.signal(&cx); }
            Context::Masked(_) => {}
        }
    }
}
#[no_mangle]
pub extern "C" fn xm1_time_delay(period: u32) -> i32 {
    let kernel = kernel();
    let Some(mut cx) = kernel.task() else { return -255; };
    let Some(task) = DelayTask::new(cx.id().get()) else { return -255; };
    DELAY_WAITERS.get()[usize::from(task.get() - 1)].wait(&mut cx, || {
        locked(|| {
            DELAYS.get().begin(task, Milliseconds(period), DELAY_TIMER.get().get().is_some());
            if DELAY_TIMER.get().get().is_none() {
                let handle = TIMERS.get().start(Milliseconds(u32::from(period != 0)), Repeat::Periodic, Some(Callback::DelayTick), &mut Handles(None)).map_err(report)?;
                DELAY_TIMER.get().set(Some(handle));
            }
            Ok::<(), Error>(())
        })
    }).map(|()| 0).unwrap_or(-255)
}

pub fn start_timer(period: Milliseconds, repeat: Repeat, callback: Option<fn()>) -> Result<TimerHandle, Error> {
    locked(|| TIMERS.get().start(period, repeat, callback.map(Callback::Owned), &mut Handles(None)))
}
pub fn cancel_timer(handle: TimerHandle) { locked(|| TIMERS.get().cancel(handle.into(), &mut Handles(None))); }
pub fn reload_timer(handle: TimerHandle, period: Milliseconds) -> Result<(), Error> {
    locked(|| TIMERS.get().set_reload(handle.into(), period, &mut Handles(None)))
}

#[no_mangle]
#[inline(never)]
extern "C" fn xm1_time_expiry_callback(callback: StockCode) { unsafe { callback.invoke() }; }
#[no_mangle]
#[inline(never)]
fn xm1_time_owned_expiry_callback(callback: fn()) { callback(); }
#[no_mangle]
#[inline(never)]
extern "C" fn xm1_time_delay_callback(callback: StockCode) { unsafe { callback.invoke() }; }
#[no_mangle]
#[inline(never)]
extern "C" fn xm1_time_timer_callback(callback: StockCode) { unsafe { callback.invoke() }; }
#[no_mangle]
#[inline(never)]
extern "C" fn xm1_time_delay_fault(callback: StockCode) { unsafe { callback.invoke() }; }
#[no_mangle]
#[inline(never)]
extern "C" fn xm1_time_delay_post_fault(callback: StockCode) { unsafe { callback.invoke() }; }
#[no_mangle]
#[inline(never)]
extern "C" fn xm1_time_timer_post_fault(callback: StockCode) { unsafe { callback.invoke() }; }
