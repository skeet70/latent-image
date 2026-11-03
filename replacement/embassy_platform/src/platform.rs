//! Phase 1: one embassy executor per domain inside a stock task, woken through the stock task
//! wakeup queue, timed by the stock tick and kernel timer counter.
//!
//! The two cores' L1 data caches are not coherent. Every block one core writes through its cache
//! is 32-byte aligned and padded, and state both cores touch goes through the uncached alias.

use core::cell::{RefCell, UnsafeCell};
use core::future::Future;
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use core::task::{Context, Poll, Waker};

use critical_section::Mutex;
use embassy_executor::raw;
use embassy_time::{Duration, Instant, Timer};
use embassy_time_queue_utils::Queue;

use crate::clock::{sleep_timeout, Clock, CounterDirection, RELOAD};
use crate::ids::{Domain, EncodedId, SlotStatus, TaskId};
use crate::kernel;
use crate::wake::{wake_path, Caller, WakePath};

#[cfg(all(feature = "counter-up", feature = "counter-down"))]
compile_error!("select one of counter-up and counter-down");
#[cfg(feature = "counter-up")]
const DIRECTION: CounterDirection = CounterDirection::Up;
#[cfg(all(feature = "counter-down", not(feature = "counter-up")))]
const DIRECTION: CounterDirection = CounterDirection::Down;
#[cfg(not(any(feature = "counter-up", feature = "counter-down")))]
compile_error!("select one of counter-up and counter-down");

/// Outside the class table at 0x7cad9c and the stack check at 0x69cc18, which cover ids 1..104;
/// `+0x0` refuses with -41 if a record already exists.
const HOST_TASK: TaskId = match TaskId::new(119) {
    Some(id) => id,
    None => panic!(),
};
const HOST_PRIORITY: u32 = 15;
const STACK_WORDS: usize = 512;
const TICKER_PERIOD: Duration = Duration::from_millis(100);
const DOORBELL_EVERY: u32 = 10;
const DOMAINS: [Domain; 2] = [Domain::A, Domain::B];
const NOT_CALLED: i32 = i32::MIN;

fn uncached<T>(pointer: *mut T) -> *mut T {
    (pointer as usize | 0x4000_0000) as *mut T
}

struct StockCpuLock;
critical_section::set_impl!(StockCpuLock);

// SAFETY: masking IRQ on the executing core excludes every other owned context of that core.
unsafe impl critical_section::Impl for StockCpuLock {
    unsafe fn acquire() -> bool {
        let was_masked = kernel::irq_masked();
        if !was_masked {
            kernel::cpu_lock(kernel::handler_context());
        }
        was_masked
    }

    unsafe fn release(was_masked: bool) {
        if !was_masked {
            kernel::cpu_unlock(kernel::handler_context());
        }
    }
}

/// Everything one domain's host task writes, touched only by that domain's core.
#[repr(C, align(32))]
struct DomainState {
    executor: UnsafeCell<MaybeUninit<raw::Executor>>,
    queue: Mutex<RefCell<Queue>>,
    clock: Mutex<RefCell<Clock>>,
    doorbell_waker: Mutex<RefCell<Option<Waker>>>,
    doorbell_delivered: Mutex<RefCell<u32>>,
    pending: AtomicBool,
    polling: AtomicBool,
    ticker: raw::TaskStorage<Ticker>,
    waiter: raw::TaskStorage<DoorbellWaiter>,
}

// SAFETY: each DomainState is used from its own domain's core only.
unsafe impl Sync for DomainState {}

impl DomainState {
    const fn new() -> DomainState {
        DomainState {
            executor: UnsafeCell::new(MaybeUninit::uninit()),
            queue: Mutex::new(RefCell::new(Queue::new())),
            clock: Mutex::new(RefCell::new(Clock::new())),
            doorbell_waker: Mutex::new(RefCell::new(None)),
            doorbell_delivered: Mutex::new(RefCell::new(0)),
            pending: AtomicBool::new(false),
            polling: AtomicBool::new(false),
            ticker: raw::TaskStorage::new(),
            waiter: raw::TaskStorage::new(),
        }
    }
}

static DOMAIN_STATE: [DomainState; 2] = [DomainState::new(), DomainState::new()];

fn state(domain: Domain) -> &'static DomainState {
    &DOMAIN_STATE[domain.index()]
}

#[repr(C, align(32))]
struct Stack(UnsafeCell<[u32; STACK_WORDS]>);

// SAFETY: a stack is written by the start hook before its task exists, then by that task only.
unsafe impl Sync for Stack {}

#[no_mangle]
static EMBASSY_STACKS: [Stack; 2] = [Stack(UnsafeCell::new([0; STACK_WORDS])), Stack(UnsafeCell::new([0; STACK_WORDS]))];

/// Word-sized: FujiMod stops on byte and halfword exclusives.
#[repr(C, align(32))]
struct LineFlag(AtomicU32);

static STARTED: LineFlag = LineFlag(AtomicU32::new(0));

/// Read and written only through the uncached alias.
#[repr(C)]
pub struct DomainRecord {
    pub entered: u32,
    pub direction_measured: u32,
    pub ticks_only: u32,
    pub ticker_iterations: u32,
    pub first_tick: u32,
    pub last_tick: u32,
    pub early_kernel_wakes: u32,
    pub late_max: u32,
    pub late_over_two_ticks: u32,
    pub sleeps_woken: u32,
    pub sleeps_timed_out: u32,
    pub sleeps_other: u32,
    pub last_sleep_other: i32,
    pub pender_task_calls: u32,
    pub pender_handler_calls: u32,
    pub pender_unavailable: u32,
    pub pender_errors: u32,
    pub last_pender_error: i32,
    pub doorbell_received: u32,
    pub doorbell_latency_max: u32,
    pub clock_regressions: u32,
    pub clock_regression_max: u32,
    pub stack_base: u32,
    pub stack_bytes: u32,
}

/// Read and written only through the uncached alias.
#[repr(C, align(32))]
pub struct Demo {
    pub magic: u32,
    pub start_calls: u32,
    pub start_skipped: u32,
    pub started: u32,
    pub panics: u32,
    /// Per domain: `+0x80`, `+0x0`, `+0xc` raw results; `NOT_CALLED` until called.
    pub start_status: [[i32; 3]; 2],
    pub doorbell_sent: u32,
    pub doorbell_seq: u32,
    pub doorbell_sent_lo: u32,
    pub doorbell_sent_hi: u32,
    pub domains: [DomainRecord; 2],
}

const DOMAIN_RECORD_ZERO: DomainRecord = DomainRecord {
    entered: 0,
    direction_measured: 0,
    ticks_only: 0,
    ticker_iterations: 0,
    first_tick: 0,
    last_tick: 0,
    early_kernel_wakes: 0,
    late_max: 0,
    late_over_two_ticks: 0,
    sleeps_woken: 0,
    sleeps_timed_out: 0,
    sleeps_other: 0,
    last_sleep_other: 0,
    pender_task_calls: 0,
    pender_handler_calls: 0,
    pender_unavailable: 0,
    pender_errors: 0,
    last_pender_error: 0,
    doorbell_received: 0,
    doorbell_latency_max: 0,
    clock_regressions: 0,
    clock_regression_max: 0,
    stack_base: 0,
    stack_bytes: 0,
};

#[no_mangle]
pub static mut EMBASSY_DEMO: Demo = Demo {
    magic: u32::from_le_bytes(*b"EMB2"),
    start_calls: 0,
    start_skipped: 0,
    started: 0,
    panics: 0,
    start_status: [[NOT_CALLED; 3]; 2],
    doorbell_sent: 0,
    doorbell_seq: 0,
    doorbell_sent_lo: 0,
    doorbell_sent_hi: 0,
    domains: [DOMAIN_RECORD_ZERO, DOMAIN_RECORD_ZERO],
};

fn demo() -> *mut Demo {
    uncached(addr_of_mut!(EMBASSY_DEMO))
}

fn record(domain: Domain) -> *mut DomainRecord {
    // SAFETY: `demo()` is valid for the payload's lifetime.
    unsafe { addr_of_mut!((*demo()).domains[domain.index()]) }
}

/// Fields of the demonstration record, through the uncached alias.
mod field {
    use core::ptr::{read_volatile, write_volatile};

    pub fn get(field: *mut u32) -> u32 {
        // SAFETY: callers pass a field of `EMBASSY_DEMO` through the uncached alias.
        unsafe { read_volatile(field) }
    }

    pub fn set(field: *mut u32, value: u32) {
        // SAFETY: callers pass a field of `EMBASSY_DEMO` through the uncached alias.
        unsafe { write_volatile(field, value) }
    }

    pub fn increment(field: *mut u32) {
        set(field, get(field).wrapping_add(1));
    }

    pub fn raise(field: *mut u32, value: u32) {
        if value > get(field) {
            set(field, value);
        }
    }
}

use field::{get, increment, raise, set};

fn saturate(value: u64) -> u32 {
    value.min(u32::MAX as u64) as u32
}

struct StockTickDriver;

fn domain_now(domain: Domain) -> u64 {
    loop {
        let before = kernel::tick();
        let count = kernel::timer_count();
        let after = kernel::tick();
        if before == after {
            let sample = critical_section::with(|cs| state(domain).clock.borrow_ref_mut(cs).now(after, count, DIRECTION));
            if sample.regression > 0 {
                let r = record(domain);
                // SAFETY: fields of the demonstration record.
                unsafe {
                    increment(addr_of_mut!((*r).clock_regressions));
                    raise(addr_of_mut!((*r).clock_regression_max), saturate(sample.regression));
                }
            }
            return sample.now;
        }
    }
}

impl embassy_time_driver::Driver for StockTickDriver {
    fn now(&self) -> u64 {
        domain_now(kernel::executing_domain())
    }

    fn schedule_wake(&self, at: u64, waker: &Waker) {
        let domain = kernel::executing_domain();
        let earlier = critical_section::with(|cs| state(domain).queue.borrow_ref_mut(cs).schedule_wake(at, waker));
        if earlier {
            pend(domain);
        }
    }
}

embassy_time_driver::time_driver_impl!(static DRIVER: StockTickDriver = StockTickDriver);

fn measure_direction(domain: Domain) {
    let measured = (0..64).find_map(|_| {
        let before = kernel::tick();
        let first = kernel::timer_count();
        let mut second = first;
        for _ in 0..256 {
            second = kernel::timer_count();
            if second != first {
                break;
            }
        }
        if kernel::tick() == before {
            CounterDirection::measured(first, second)
        } else {
            None
        }
    });
    let r = record(domain);
    // SAFETY: fields of the demonstration record.
    unsafe {
        set(addr_of_mut!((*r).direction_measured), match measured {
            Some(CounterDirection::Up) => 1,
            Some(CounterDirection::Down) => 2,
            None => 0,
        });
    }
    if measured != Some(DIRECTION) {
        critical_section::with(|cs| state(domain).clock.borrow_ref_mut(cs).use_ticks_only());
        unsafe { set(addr_of_mut!((*r).ticks_only), 1) };
    }
}

fn domain_of_context(context: *mut ()) -> Domain {
    if context as usize == 1 {
        Domain::A
    } else {
        Domain::B
    }
}

const fn context_of(domain: Domain) -> *mut () {
    (domain.index() + 1) as *mut ()
}

#[export_name = "__pender"]
fn pender(context: *mut ()) {
    pend(domain_of_context(context));
}

fn pend(target: Domain) {
    let domain = kernel::executing_domain();
    let caller = Caller {
        domain,
        handler_context: kernel::handler_context(),
        irq_masked: kernel::irq_masked(),
        in_host_poll_of: if state(domain).polling.load(Ordering::Acquire) { Some(domain) } else { None },
    };
    if domain == target {
        state(target).pending.store(true, Ordering::Release);
    }
    let r = record(target);
    let id = EncodedId::new(target, HOST_TASK);
    // SAFETY (each block below): fields of the demonstration record.
    let status = match wake_path(target, caller) {
        WakePath::None => return,
        WakePath::Unavailable => {
            unsafe { increment(addr_of_mut!((*r).pender_unavailable)) };
            return;
        }
        _ if cfg!(feature = "control-no-pender-kernel") => {
            unsafe { increment(addr_of_mut!((*r).pender_unavailable)) };
            return;
        }
        WakePath::Task => {
            unsafe { increment(addr_of_mut!((*r).pender_task_calls)) };
            kernel::wakeup(id)
        }
        WakePath::Handler => {
            unsafe { increment(addr_of_mut!((*r).pender_handler_calls)) };
            kernel::wakeup_from_handler(id)
        }
    };
    match status {
        SlotStatus::Ok | SlotStatus::QueueOverflow => {}
        other => unsafe {
            increment(addr_of_mut!((*r).pender_errors));
            set(addr_of_mut!((*r).last_pender_error) as *mut u32, raw_status(other) as u32);
        },
    }
}

fn raw_status(status: SlotStatus) -> i32 {
    match status {
        SlotStatus::Ok => 0,
        SlotStatus::Parameter => -17,
        SlotStatus::Id => -18,
        SlotStatus::Context => -25,
        SlotStatus::IllegalUse => -28,
        SlotStatus::ObjectState => -41,
        SlotStatus::NotExisting => -42,
        SlotStatus::QueueOverflow => -43,
        SlotStatus::WaitReleased => -49,
        SlotStatus::Timeout => -50,
        SlotStatus::Other(value) => value,
    }
}

fn region<T>(value: &'static T) -> (usize, usize) {
    (value as *const T as usize, core::mem::size_of::<T>())
}

extern "C" fn host_main(argument: u32) -> ! {
    let domain = if argument == 0 { Domain::A } else { Domain::B };
    let own = state(domain);
    for (start, bytes) in [region(own), region(&EMBASSY_STACKS[domain.index()])] {
        kernel::clean_dcache(start, bytes);
    }
    let r = record(domain);
    // SAFETY (each block in this function): fields of the demonstration record.
    unsafe { set(addr_of_mut!((*r).entered), 1) };
    measure_direction(domain);
    // SAFETY: only this task touches its domain's executor slot, once, before any poll.
    let executor: &'static raw::Executor = unsafe { (*own.executor.get()).write(raw::Executor::new(context_of(domain))) };
    let spawner = executor.spawner();
    own.polling.store(true, Ordering::Release);
    spawner.spawn(own.ticker.spawn(|| Ticker::new(domain)).unwrap());
    if domain == Domain::B {
        spawner.spawn(own.waiter.spawn(|| DoorbellWaiter { consumed: 0 }).unwrap());
    }
    loop {
        own.pending.store(false, Ordering::Release);
        own.polling.store(true, Ordering::Release);
        // SAFETY: this loop is the only caller of poll for this executor, and the pender never polls.
        unsafe { executor.poll() };
        let now = domain_now(domain);
        let next = critical_section::with(|cs| own.queue.borrow_ref_mut(cs).next_expiration(now));
        own.polling.store(false, Ordering::Release);
        if own.pending.load(Ordering::Acquire) {
            continue;
        }
        match kernel::sleep(sleep_timeout(now, next)) {
            SlotStatus::Ok => {
                unsafe { increment(addr_of_mut!((*r).sleeps_woken)) };
                own.polling.store(true, Ordering::Release);
                deliver_doorbell(domain);
            }
            SlotStatus::Timeout => unsafe {
                increment(addr_of_mut!((*r).sleeps_timed_out));
                if domain_now(domain) < next {
                    increment(addr_of_mut!((*r).early_kernel_wakes));
                }
            },
            other => unsafe {
                increment(addr_of_mut!((*r).sleeps_other));
                set(addr_of_mut!((*r).last_sleep_other) as *mut u32, raw_status(other) as u32);
            },
        }
    }
}

/// Every `TICKER_PERIOD`: records the wake against its deadline; domain A rings domain B's
/// doorbell every `DOORBELL_EVERY` iterations.
struct Ticker {
    domain: Domain,
    deadline: Instant,
    timer: Timer,
}

impl Ticker {
    fn new(domain: Domain) -> Ticker {
        let deadline = Instant::now() + TICKER_PERIOD;
        Ticker { domain, deadline, timer: Timer::at(deadline) }
    }
}

impl Future for Ticker {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        loop {
            if Pin::new(&mut this.timer).poll(cx).is_pending() {
                return Poll::Pending;
            }
            let now = Instant::now();
            let tick = kernel::tick();
            let r = record(this.domain);
            let late = (now - this.deadline).as_ticks();
            // SAFETY: fields of the demonstration record.
            unsafe {
                raise(addr_of_mut!((*r).late_max), saturate(late));
                if late > 2 * RELOAD as u64 {
                    increment(addr_of_mut!((*r).late_over_two_ticks));
                }
                if get(addr_of_mut!((*r).ticker_iterations)) == 0 {
                    set(addr_of_mut!((*r).first_tick), tick);
                }
                set(addr_of_mut!((*r).last_tick), tick);
                increment(addr_of_mut!((*r).ticker_iterations));
                if this.domain == Domain::A && get(addr_of_mut!((*r).ticker_iterations)) % DOORBELL_EVERY == 0 {
                    ring_doorbell(now.as_ticks());
                }
            }
            this.deadline += TICKER_PERIOD;
            this.timer = Timer::at(this.deadline);
        }
    }
}

fn ring_doorbell(now: u64) {
    let d = demo();
    // SAFETY: fields of the demonstration record; domain A is the only writer.
    unsafe {
        set(addr_of_mut!((*d).doorbell_sent_lo), now as u32);
        set(addr_of_mut!((*d).doorbell_sent_hi), (now >> 32) as u32);
        set(addr_of_mut!((*d).doorbell_seq), get(addr_of_mut!((*d).doorbell_seq)).wrapping_add(1));
        increment(addr_of_mut!((*d).doorbell_sent));
    }
    pend(Domain::B);
}

/// Called only after domain B's host task was woken through the kernel.
fn deliver_doorbell(domain: Domain) {
    if domain != Domain::B {
        return;
    }
    let d = demo();
    // SAFETY: a field of the demonstration record.
    let seq = unsafe { get(addr_of_mut!((*d).doorbell_seq)) };
    let own = state(domain);
    let waker = critical_section::with(|cs| {
        let mut delivered = own.doorbell_delivered.borrow_ref_mut(cs);
        if *delivered == seq {
            return None;
        }
        *delivered = seq;
        own.doorbell_waker.borrow_ref_mut(cs).take()
    });
    if let Some(waker) = waker {
        waker.wake();
    }
}

struct DoorbellWaiter {
    consumed: u32,
}

impl Future for DoorbellWaiter {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        let own = state(Domain::B);
        let delivered = critical_section::with(|cs| {
            let delivered = *own.doorbell_delivered.borrow_ref(cs);
            if delivered == this.consumed {
                *own.doorbell_waker.borrow_ref_mut(cs) = Some(cx.waker().clone());
            }
            delivered
        });
        if delivered != this.consumed {
            this.consumed = delivered;
            let d = demo();
            let r = record(Domain::B);
            // SAFETY: fields of the demonstration record.
            unsafe {
                let sent = (get(addr_of_mut!((*d).doorbell_sent_hi)) as u64) << 32 | get(addr_of_mut!((*d).doorbell_sent_lo)) as u64;
                raise(addr_of_mut!((*r).doorbell_latency_max), saturate(Instant::now().as_ticks().saturating_sub(sent)));
                increment(addr_of_mut!((*r).doorbell_received));
            }
            cx.waker().wake_by_ref();
        }
        Poll::Pending
    }
}

extern "C" {
    static __ram_push_start: u8;
    static __experiment_code_end: u8;
}

/// Experiment hook in front of the stock modal dispatcher 0x00bfb028; the first call from task
/// context with IRQ unmasked creates and activates both host tasks.
#[no_mangle]
pub extern "C" fn embassy_on_dispatch(_registers: *mut u32) {
    let d = demo();
    // SAFETY (each block in this function): fields of the demonstration record.
    unsafe { increment(addr_of_mut!((*d).start_calls)) };
    if STARTED.0.load(Ordering::Acquire) != 0 {
        return;
    }
    if kernel::handler_context() || kernel::irq_masked() {
        unsafe { increment(addr_of_mut!((*d).start_skipped)) };
        return;
    }
    if STARTED.0.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_err() {
        return;
    }
    unsafe { set(addr_of_mut!((*d).started), 1) };
    if cfg!(feature = "control-no-executors") {
        return;
    }
    for domain in DOMAINS {
        let stack = EMBASSY_STACKS[domain.index()].0.get() as *mut u32;
        for word in 0..STACK_WORDS {
            // SAFETY: the stack's task does not exist yet.
            unsafe { write_volatile(stack.add(word), HOST_TASK.fill_word()) };
        }
        let r = record(domain);
        unsafe {
            set(addr_of_mut!((*r).stack_base), stack as u32);
            set(addr_of_mut!((*r).stack_bytes), (STACK_WORDS * 4) as u32);
        }
    }
    let (start, end) = (addr_of!(__ram_push_start) as usize, addr_of!(__experiment_code_end) as usize);
    kernel::clean_dcache(start, end - start);
    {
        let _pair = kernel::CriticalPair::enter();
        for domain in DOMAINS {
            let packet = kernel::CreatePacket {
                flags: 2,
                argument: domain.index() as u32,
                entry: host_main as *const () as usize as u32,
                priority: HOST_PRIORITY,
                stack_bytes: (STACK_WORDS * 4) as u32,
                stack_base: EMBASSY_STACKS[domain.index()].0.get() as u32,
            };
            let status = unsafe { addr_of_mut!((*d).start_status[domain.index()]) };
            unsafe {
                write_volatile(addr_of_mut!((*status)[0]), raw_status(kernel::register_auxiliary(domain, HOST_TASK)));
                write_volatile(addr_of_mut!((*status)[1]), raw_status(kernel::create(domain, HOST_TASK, &packet)));
            }
        }
    }
    for domain in DOMAINS {
        let status = unsafe { addr_of_mut!((*d).start_status[domain.index()]) };
        unsafe {
            if read_volatile(addr_of!((*status)[1])) == 0 {
                write_volatile(addr_of_mut!((*status)[2]), raw_status(kernel::activate(EncodedId::new(domain, HOST_TASK))));
            }
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    // SAFETY: a field of the demonstration record.
    unsafe { increment(addr_of_mut!((*demo()).panics)) };
    if !kernel::irq_masked() && !kernel::handler_context() {
        loop {
            kernel::sleep(-1);
        }
    }
    loop {
        core::hint::spin_loop();
    }
}
