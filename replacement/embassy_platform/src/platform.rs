//! Phase 1: one embassy executor per domain inside a stock task, woken through the stock task
//! wakeup queue, timed by the stock tick and kernel timer counter.

use core::cell::{RefCell, UnsafeCell};
use core::future::poll_fn;
use core::mem::MaybeUninit;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Poll, Waker};

use critical_section::{CriticalSection, Mutex};
use embassy_executor::raw;
use embassy_time::{Duration, Instant, Timer};
use embassy_time_queue_utils::Queue;

use crate::clock::{sleep_timeout, Clock, CounterDirection};
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

/// Absent from every stock creation path: the class table at 0x7cad9c covers ids 1..104 and the
/// stack check at 0x69cc18 cycles through ids 1..104 only.
const HOST_TASK: TaskId = match TaskId::new(119) {
    Some(id) => id,
    None => panic!(),
};
const HOST_PRIORITY: u32 = 15;
const STACK_WORDS: usize = 512;
const TICKER_PERIOD: Duration = Duration::from_millis(100);
const DOORBELL_EVERY: u32 = 10;
const DOMAINS: [Domain; 2] = [Domain::A, Domain::B];

fn uncached<T>(pointer: *mut T) -> *mut T {
    (pointer as usize | 0x4000_0000) as *mut T
}

struct StockCpuLock;
critical_section::set_impl!(StockCpuLock);

// SAFETY: masking IRQ on the executing core excludes every other owned context of that core;
// owned state is per domain, so nothing is shared with the other core under this lock.
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

struct Host {
    executor: UnsafeCell<MaybeUninit<raw::Executor>>,
    queue: Mutex<RefCell<Queue>>,
    clock: Mutex<RefCell<Clock>>,
    doorbell_waker: Mutex<RefCell<Option<Waker>>>,
    doorbell_delivered: Mutex<RefCell<u32>>,
    pending: AtomicBool,
    polling: AtomicBool,
}

// SAFETY: each Host is touched only from its own domain's core, apart from the executor reference
// the pender never dereferences.
unsafe impl Sync for Host {}

impl Host {
    const fn new() -> Host {
        Host {
            executor: UnsafeCell::new(MaybeUninit::uninit()),
            queue: Mutex::new(RefCell::new(Queue::new())),
            clock: Mutex::new(RefCell::new(Clock::new())),
            doorbell_waker: Mutex::new(RefCell::new(None)),
            doorbell_delivered: Mutex::new(RefCell::new(0)),
            pending: AtomicBool::new(false),
            polling: AtomicBool::new(false),
        }
    }
}

static HOSTS: [Host; 2] = [Host::new(), Host::new()];

fn host(domain: Domain) -> &'static Host {
    &HOSTS[domain.index()]
}

#[repr(C, align(8))]
struct Stacks([[u32; STACK_WORDS]; 2]);
#[no_mangle]
static mut EMBASSY_STACKS: Stacks = Stacks([[0; STACK_WORDS]; 2]);

/// Read through the uncached alias, so the other core and `read-ram` see current values.
#[repr(C)]
pub struct DomainRecord {
    pub entered: u32,
    pub direction_measured: u32,
    pub ticks_only: u32,
    pub ticker_iterations: u32,
    pub first_tick: u32,
    pub last_tick: u32,
    pub early_wakes: u32,
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
    pub stack_base: u32,
    pub stack_bytes: u32,
}

#[repr(C)]
pub struct Demo {
    pub magic: u32,
    pub start_calls: u32,
    pub start_skipped: u32,
    pub started: u32,
    /// Per domain: `+0x80`, `+0x0`, `+0xc` raw results.
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
    early_wakes: 0,
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
    stack_base: 0,
    stack_bytes: 0,
};

#[no_mangle]
pub static mut EMBASSY_DEMO: Demo = Demo {
    magic: u32::from_le_bytes(*b"EMB1"),
    start_calls: 0,
    start_skipped: 0,
    started: 0,
    start_status: [[0; 3]; 2],
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
    // SAFETY: `demo()` is a valid pointer for the payload's lifetime.
    unsafe { addr_of_mut!((*demo()).domains[domain.index()]) }
}

fn get(field: *mut u32) -> u32 {
    // SAFETY: fields of the demonstration record.
    unsafe { read_volatile(field) }
}

fn set(field: *mut u32, value: u32) {
    // SAFETY: fields of the demonstration record.
    unsafe { write_volatile(field, value) }
}

fn increment(field: *mut u32) {
    set(field, get(field).wrapping_add(1));
}

fn raise(field: *mut u32, value: u32) {
    if value > get(field) {
        set(field, value);
    }
}

struct StockTickDriver;

fn domain_now(domain: Domain) -> u64 {
    loop {
        let before = kernel::tick();
        let count = kernel::timer_count();
        let after = kernel::tick();
        if before == after {
            return critical_section::with(|cs| host(domain).clock.borrow_ref_mut(cs).now(after, count, DIRECTION));
        }
    }
}

impl embassy_time_driver::Driver for StockTickDriver {
    fn now(&self) -> u64 {
        domain_now(kernel::executing_domain())
    }

    fn schedule_wake(&self, at: u64, waker: &Waker) {
        let domain = kernel::executing_domain();
        let earlier = critical_section::with(|cs| host(domain).queue.borrow_ref_mut(cs).schedule_wake(at, waker));
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
    // SAFETY: fields of the demonstration record.
    let r = record(domain);
    unsafe {
        set(addr_of_mut!((*r).direction_measured), match measured {
            Some(CounterDirection::Up) => 1,
            Some(CounterDirection::Down) => 2,
            None => 0,
        });
    }
    if measured != Some(DIRECTION) {
        critical_section::with(|cs| host(domain).clock.borrow_ref_mut(cs).use_ticks_only());
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
        in_host_poll_of: if host(domain).polling.load(Ordering::Acquire) { Some(domain) } else { None },
    };
    if domain == target {
        host(target).pending.store(true, Ordering::Release);
    }
    let r = record(target);
    let id = EncodedId::new(target, HOST_TASK);
    let status = match wake_path(target, caller) {
        WakePath::None => return,
        WakePath::Unavailable => {
            // SAFETY: fields of the demonstration record.
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

extern "C" fn host_main(argument: u32) -> ! {
    let domain = if argument == 0 { Domain::A } else { Domain::B };
    let state = host(domain);
    let r = record(domain);
    unsafe { set(addr_of_mut!((*r).entered), 1) };
    measure_direction(domain);
    // SAFETY: only this task touches its domain's executor slot, once, before any poll.
    let executor: &'static raw::Executor = unsafe { (*state.executor.get()).write(raw::Executor::new(context_of(domain))) };
    let spawner = executor.spawner();
    spawner.spawn(ticker(domain).unwrap());
    if domain == Domain::B {
        spawner.spawn(doorbell_waiter().unwrap());
    }
    loop {
        state.pending.store(false, Ordering::Release);
        state.polling.store(true, Ordering::Release);
        // SAFETY: this loop is the only caller of poll for this executor, and the pender never polls.
        unsafe { executor.poll() };
        let now = domain_now(domain);
        let next = critical_section::with(|cs| state.queue.borrow_ref_mut(cs).next_expiration(now));
        state.polling.store(false, Ordering::Release);
        if state.pending.load(Ordering::Acquire) {
            continue;
        }
        match kernel::sleep(sleep_timeout(now, next)) {
            SlotStatus::Ok => {
                unsafe { increment(addr_of_mut!((*r).sleeps_woken)) };
                deliver_doorbell(domain);
            }
            SlotStatus::Timeout => unsafe { increment(addr_of_mut!((*r).sleeps_timed_out)) },
            other => unsafe {
                increment(addr_of_mut!((*r).sleeps_other));
                set(addr_of_mut!((*r).last_sleep_other) as *mut u32, raw_status(other) as u32);
            },
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn ticker(domain: Domain) {
    let r = record(domain);
    let mut deadline = Instant::now() + TICKER_PERIOD;
    loop {
        Timer::at(deadline).await;
        let now = Instant::now();
        let tick = kernel::tick();
        // SAFETY: fields of the demonstration record.
        unsafe {
            if now < deadline {
                increment(addr_of_mut!((*r).early_wakes));
            } else {
                let late = (now - deadline).as_ticks();
                raise(addr_of_mut!((*r).late_max), late.min(u32::MAX as u64) as u32);
                if late > 2 * crate::clock::RELOAD as u64 {
                    increment(addr_of_mut!((*r).late_over_two_ticks));
                }
            }
            if get(addr_of_mut!((*r).ticker_iterations)) == 0 {
                set(addr_of_mut!((*r).first_tick), tick);
            }
            set(addr_of_mut!((*r).last_tick), tick);
            increment(addr_of_mut!((*r).ticker_iterations));
            if domain == Domain::A && get(addr_of_mut!((*r).ticker_iterations)) % DOORBELL_EVERY == 0 {
                ring_doorbell(now.as_ticks());
            }
        }
        deadline += TICKER_PERIOD;
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

/// Hands a doorbell to domain B's waiter after its host task was woken through the kernel.
fn deliver_doorbell(domain: Domain) {
    if domain != Domain::B {
        return;
    }
    let d = demo();
    let seq = unsafe { get(addr_of_mut!((*d).doorbell_seq)) };
    critical_section::with(|cs| {
        let state = host(domain);
        let mut delivered = state.doorbell_delivered.borrow_ref_mut(cs);
        if *delivered != seq {
            *delivered = seq;
            if let Some(waker) = state.doorbell_waker.borrow_ref_mut(cs).take() {
                waker.wake();
            }
        }
    });
}

#[embassy_executor::task]
async fn doorbell_waiter() {
    let state = host(Domain::B);
    let r = record(Domain::B);
    let d = demo();
    let mut consumed = 0u32;
    loop {
        consumed = poll_fn(|cx| {
            critical_section::with(|cs: CriticalSection| {
                let delivered = *state.doorbell_delivered.borrow_ref(cs);
                if delivered != consumed {
                    Poll::Ready(delivered)
                } else {
                    *state.doorbell_waker.borrow_ref_mut(cs) = Some(cx.waker().clone());
                    Poll::Pending
                }
            })
        })
        .await;
        // SAFETY: fields of the demonstration record.
        unsafe {
            let sent = (get(addr_of_mut!((*d).doorbell_sent_hi)) as u64) << 32 | get(addr_of_mut!((*d).doorbell_sent_lo)) as u64;
            let latency = Instant::now().as_ticks().saturating_sub(sent);
            raise(addr_of_mut!((*r).doorbell_latency_max), latency.min(u32::MAX as u64) as u32);
            increment(addr_of_mut!((*r).doorbell_received));
        }
    }
}

extern "C" {
    static __ram_push_start: u8;
    static __experiment_code_end: u8;
}

static STARTED: AtomicBool = AtomicBool::new(false);

/// Experiment hook in front of the stock modal dispatcher 0x00bfb028; the first call from task
/// context with IRQ unmasked creates and activates both host tasks.
#[no_mangle]
pub extern "C" fn embassy_on_dispatch(_registers: *mut u32) {
    let d = demo();
    // SAFETY: fields of the demonstration record.
    unsafe { increment(addr_of_mut!((*d).start_calls)) };
    if STARTED.load(Ordering::Acquire) {
        return;
    }
    if kernel::handler_context() || kernel::irq_masked() {
        unsafe { increment(addr_of_mut!((*d).start_skipped)) };
        return;
    }
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    unsafe { set(addr_of_mut!((*d).started), 1) };
    if cfg!(feature = "control-no-executors") {
        return;
    }
    let (start, end) = (addr_of!(__ram_push_start) as usize, addr_of!(__experiment_code_end) as usize);
    for domain in DOMAINS {
        // SAFETY: each stack belongs to one domain's host task, which does not run yet.
        let stack = unsafe { addr_of_mut!(EMBASSY_STACKS.0[domain.index()]) as *mut u32 };
        for word in 0..STACK_WORDS {
            unsafe { write_volatile(uncached(stack.add(word)), HOST_TASK.fill_word()) };
        }
        let packet = kernel::CreatePacket {
            flags: 2,
            argument: domain.index() as u32,
            entry: host_main as *const () as usize as u32,
            priority: HOST_PRIORITY,
            stack_bytes: (STACK_WORDS * 4) as u32,
            stack_base: stack as u32,
        };
        let r = record(domain);
        unsafe {
            set(addr_of_mut!((*r).stack_base), stack as u32);
            set(addr_of_mut!((*r).stack_bytes), (STACK_WORDS * 4) as u32);
            (*d).start_status[domain.index()][0] = raw_status(kernel::register_auxiliary(domain, HOST_TASK));
            (*d).start_status[domain.index()][1] = raw_status(kernel::create(domain, HOST_TASK, &packet));
        }
    }
    kernel::clean_dcache(start, end - start);
    for domain in DOMAINS {
        unsafe {
            if (*d).start_status[domain.index()][1] == 0 {
                (*d).start_status[domain.index()][2] = raw_status(kernel::activate(EncodedId::new(domain, HOST_TASK)));
            }
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
