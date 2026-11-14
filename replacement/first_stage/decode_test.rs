//! The RAM-loaded decode test: drives the boot's staged image-4 record through the
//! 0xfff43000 engine inside stock's own reservation bracket, with the first stage's driver
//! and feed boundaries, into the retired image-5 staging extent, and records what each job did.

use core::ptr::{addr_of_mut, read_volatile};

use crate::decode_test_crc::{finish, start, update};
use crate::decode_test_stock::{self, Reservation};
use crate::engine::{
    self, Bus, ClockRegister, Destination, EngineError, EngineRegister, Register, Session, Source,
    VolatileBus,
};
use crate::format::UNIT_BYTES;
use crate::images::CompressedHeader;

/// The boot's image-4 staging base, the same value `images::SCRATCH` holds.
const STREAM: u32 = 0x42e4_0000;
/// The staged extent: eleven whole 128 KiB units.
const STREAM_EXTENT: u32 = 0x0016_0000;
/// Header word 1 of the pinned 1.50 image-4 record.
const RECORD_LENGTH: u32 = 0x0014_6567;
/// CRC-32 over `[STREAM, STREAM + RECORD_LENGTH)`.
const RECORD_CRC: u32 = 0x2015_9a32;
/// Header word 0 of the same record.
const DECODED_LENGTH: u32 = 0x0028_2800;
/// CRC-32 over the pinned 1.50 image-4 decode.
const DECODED_CRC: u32 = 0x21a1_a3e5;
/// The image-5 staging extent; factory automation also reads scripts at 0x43e6ff00.
const DESTINATION: u32 = 0x43e4_0000;
const DESTINATION_CAPACITY: u32 = 0x002e_0000;
/// The boot's decoded image 4.
const IMAGE4: u32 = 0x4061_c400;
/// The kernel timer counter the boot log timestamps from.
const TIMER: u32 = 0xfff9_1000;
const TIMER_TICKS: u32 = 0x400c_0800;
const TIMER_PERIOD: u32 = 30_000;
/// The stock modal dispatcher every key press reaches.
const TRIGGER_SITE: u32 = 0x00bf_b028;
const UNCACHED: u32 = 0x4000_0000;
const JOB_SLOTS: usize = 11;
const HEADER_WORDS: usize = 5;
const ENGINE_WORDS: usize = 8;
/// Polls of 0xfff43000 before the reset is refused; the stock wait at 0x00683958 has none.
const RESET_POLL_LIMIT: u32 = 1_000_000;

const STATE_IDLE: u32 = 0;
const STATE_RUNNING: u32 = 1;
const STATE_COMPLETED: u32 = 2;
const STATE_REFUSED_STREAM: u32 = 3;
const STATE_REFUSED_HEADER: u32 = 4;
const STATE_REFUSED_LENGTH: u32 = 5;
const STATE_ENGINE_FAULT: u32 = 6;
const STATE_FEED_ERROR: u32 = 7;
const STATE_OUTPUT_LENGTH: u32 = 8;
const STATE_JOB_OVERFLOW: u32 = 9;
const STATE_OUTPUT_OVERFLOW: u32 = 10;
/// Acquire did not yield a new clock handle; stock may still hold the reservation and route.
const STATE_REFUSED_RESERVATION: u32 = 11;
/// The reservation was released before the refusal.
const STATE_REFUSED_RESET: u32 = 12;
const STATE_REFUSED_CLOCK_ROUTE: u32 = 13;

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Job {
    pub unit: u32,
    pub available: u32,
    pub feed_end: u32,
    pub source: u32,
    pub length: u32,
    pub destination: u32,
    pub destination_readback: u32,
    pub output_count: u32,
    pub status_after_trigger: u32,
    pub status_reads: u32,
    pub tick_start: u32,
    pub tick_end: u32,
    pub produced: u32,
}

const ZERO_JOB: Job = Job {
    unit: 0,
    available: 0,
    feed_end: 0,
    source: 0,
    length: 0,
    destination: 0,
    destination_readback: 0,
    output_count: 0,
    status_after_trigger: 0,
    status_reads: 0,
    tick_start: 0,
    tick_end: 0,
    produced: 0,
};

#[repr(C)]
pub struct Record {
    pub magic: u32,
    pub state: u32,
    pub verdict: u32,
    pub handler_calls: u32,
    pub core_identity: u32,
    pub trigger_site: u32,
    pub header: [u32; HEADER_WORDS],
    pub stream_length: u32,
    pub stream_digest: u32,
    pub decoded_length: u32,
    pub destination: u32,
    pub capacity: u32,
    pub jobs: u32,
    pub produced: u32,
    pub engine_before: [u32; ENGINE_WORDS],
    pub clock_before: [u32; 2],
    pub engine_after: [u32; ENGINE_WORDS],
    pub clock_after: [u32; 2],
    pub reservation_before: u32,
    pub reservation_acquired: u32,
    pub reservation_released: u32,
    pub resource_state_before: u32,
    pub resource_state_after: u32,
    pub reset_polls: u32,
    pub acquire_ticks: u32,
    pub reset_ticks: u32,
    pub refusals: u32,
    pub output_digest: u32,
    pub image_digest: u32,
    pub mismatches: u32,
    pub first_mismatch: u32,
    pub last_mismatch: u32,
    pub tick_start: u32,
    pub tick_end: u32,
    pub digest_ticks: u32,
    pub jobs_data: [Job; JOB_SLOTS],
    pub clock_held: u32,
    pub route_held: u32,
    pub route_after: u32,
    pub clock_closed: u32,
    pub timer_tick_cells: [u32; 30],
}

/// 928 bytes; camera `read-ram` requires four windows.
#[no_mangle]
pub static mut DECODE_TEST_RECORD: Record = Record {
    magic: u32::from_le_bytes(*b"DTST"),
    state: STATE_IDLE,
    verdict: 0,
    handler_calls: 0,
    core_identity: 0,
    trigger_site: TRIGGER_SITE,
    header: [0; HEADER_WORDS],
    stream_length: 0,
    stream_digest: 0,
    decoded_length: 0,
    destination: DESTINATION,
    capacity: DESTINATION_CAPACITY,
    jobs: 0,
    produced: 0,
    engine_before: [0; ENGINE_WORDS],
    clock_before: [0; 2],
    engine_after: [0; ENGINE_WORDS],
    clock_after: [0; 2],
    reservation_before: 0,
    reservation_acquired: 0,
    reservation_released: 0,
    resource_state_before: 0,
    resource_state_after: 0,
    reset_polls: 0,
    acquire_ticks: 0,
    reset_ticks: 0,
    refusals: 0,
    output_digest: 0,
    image_digest: 0,
    mismatches: 0,
    first_mismatch: u32::MAX,
    last_mismatch: 0,
    tick_start: 0,
    tick_end: 0,
    digest_ticks: 0,
    jobs_data: [ZERO_JOB; JOB_SLOTS],
    clock_held: 0,
    route_held: 0,
    route_after: 0,
    clock_closed: 0,
    timer_tick_cells: [0; 30],
};

/// Per-job register traffic the driver's `Session` produces, kept in the record.
struct Probe {
    inner: VolatileBus,
    record: *mut Record,
    job: usize,
    triggered: bool,
    started: bool,
}

impl Bus for Probe {
    fn read(&mut self, register: Register) -> u32 {
        let value = self.inner.read(register);
        if register == Register::Engine(EngineRegister::Status) && self.job < JOB_SLOTS {
            unsafe {
                if self.triggered {
                    (*self.record).jobs_data[self.job].status_after_trigger = value;
                    self.triggered = false;
                }
                (*self.record).jobs_data[self.job].status_reads += 1;
            }
        }
        value
    }

    fn write(&mut self, register: Register, value: u32) {
        self.inner.write(register, value);
        if register == Register::Engine(EngineRegister::Source) && self.started {
            self.job += 1;
            self.started = false;
        }
        if self.job < JOB_SLOTS {
            unsafe {
                match register {
                    Register::Engine(EngineRegister::Source) => (*self.record).jobs_data[self.job].source = value,
                    Register::Engine(EngineRegister::Length) => (*self.record).jobs_data[self.job].length = value,
                    Register::Engine(EngineRegister::Destination) => (*self.record).jobs_data[self.job].destination = value,
                    Register::Engine(EngineRegister::Start) => {
                        self.triggered = true;
                        self.started = true;
                    }
                    _ => {}
                }
            }
        }
    }
}

fn uncached<T>(pointer: *mut T) -> *mut T {
    ((pointer as usize) | UNCACHED as usize) as *mut T
}

unsafe fn tick(record: *mut Record, sample: usize) -> u32 {
    loop {
        let ticks = read_volatile(TIMER_TICKS as *const u32);
        let counter = read_volatile(TIMER as *const u32);
        if ticks == read_volatile(TIMER_TICKS as *const u32) {
            (*record).timer_tick_cells[sample] = ticks;
            return ticks.wrapping_mul(TIMER_PERIOD).wrapping_add(counter);
        }
    }
}

unsafe fn core_identity() -> u32 {
    let identity: u32;
    core::arch::asm!("mrc p14, 0, {0}, c2, c0, 0", out(reg) identity);
    identity
}

fn engine_word(register: EngineRegister) -> u32 {
    unsafe { read_volatile((engine::ENGINE_BASE + register as u32) as *const u32) }
}

fn clock_word(register: ClockRegister) -> u32 {
    unsafe { read_volatile((engine::CLOCK_BASE + register as u32) as *const u32) }
}

/// The count register 0xfff4304c is omitted: the model refuses it while a MOVRSZ job exists.
fn engine_words() -> [u32; ENGINE_WORDS] {
    [
        engine_word(EngineRegister::Reset),
        engine_word(EngineRegister::Start),
        engine_word(EngineRegister::Source),
        engine_word(EngineRegister::Length),
        engine_word(EngineRegister::Destination),
        engine_word(EngineRegister::Control),
        engine_word(EngineRegister::Status),
        engine_word(EngineRegister::Enable),
    ]
}

fn clock_words() -> [u32; 2] {
    [clock_word(ClockRegister::Gate), clock_word(ClockRegister::Enable)]
}

fn handle_word() -> u32 {
    decode_test_stock::handle().map_or(0, decode_test_stock::ClockHandle::word)
}

/// The bookkeeping `hw_fff430_release` 0x006838dc undoes, without its enable write at 0x006838e8.
fn release_bookkeeping(reservation: Reservation) {
    decode_test_stock::release_clock(reservation);
    decode_test_stock::clear_route();
    decode_test_stock::signal_resource();
}

fn reset_engine() -> Option<u32> {
    let mut bus = VolatileBus;
    bus.write(Register::Engine(EngineRegister::Reset), 0);
    let mut polls = 0;
    while polls < RESET_POLL_LIMIT {
        if bus.read(Register::Engine(EngineRegister::Reset)) & 1 != 0 {
            return Some(polls);
        }
        polls += 1;
    }
    None
}

unsafe fn crc32_range(address: u32, length: u32) -> u32 {
    let base = address as *const u32;
    let words = (length / 4) as usize;
    let mut crc = start();
    let mut index = 0;
    while index < words {
        let word = read_volatile(base.add(index));
        crc = update(crc, word as u8);
        crc = update(crc, (word >> 8) as u8);
        crc = update(crc, (word >> 16) as u8);
        crc = update(crc, (word >> 24) as u8);
        index += 1;
    }
    let tail = length % 4;
    if tail != 0 {
        let word = read_volatile(base.add(words));
        let mut byte = 0;
        while byte < tail {
            crc = update(crc, (word >> (8 * byte)) as u8);
            byte += 1;
        }
    }
    finish(crc)
}

/// Byte offsets of the first and last difference and the number of differing bytes.
unsafe fn compare_ranges(left: u32, right: u32, length: u32) -> (u32, u32, u32) {
    let left = left as *const u32;
    let right = right as *const u32;
    let words = (length / 4) as usize;
    let mut mismatches = 0;
    let mut first = u32::MAX;
    let mut last = 0;
    let mut index = 0;
    while index < words {
        let a = read_volatile(left.add(index));
        let b = read_volatile(right.add(index));
        if a != b {
            let mut byte = 0;
            while byte < 4 {
                if (a >> (8 * byte)) & 0xff != (b >> (8 * byte)) & 0xff {
                    let offset = (index as u32) * 4 + byte as u32;
                    mismatches += 1;
                    if first == u32::MAX {
                        first = offset;
                    }
                    last = offset;
                }
                byte += 1;
            }
        }
        index += 1;
    }
    let mut byte = (words as u32) * 4;
    while byte < length {
        let a = read_volatile((left as *const u8).add(byte as usize));
        let b = read_volatile((right as *const u8).add(byte as usize));
        if a != b {
            mismatches += 1;
            if first == u32::MAX {
                first = byte;
            }
            last = byte;
        }
        byte += 1;
    }
    (mismatches, first, last)
}

#[no_mangle]
pub extern "C" fn xm1_decode_test_on_dispatch(_registers: *mut u32) {
    let record = uncached(addr_of_mut!(DECODE_TEST_RECORD));
    unsafe {
        (*record).handler_calls = (*record).handler_calls.wrapping_add(1);
        if (*record).state != STATE_IDLE {
            return;
        }
        (*record).state = STATE_RUNNING;
        (*record).core_identity = core_identity();
        (*record).tick_start = tick(record, 0);

        (*record).stream_length = RECORD_LENGTH;
        (*record).stream_digest = crc32_range(STREAM, RECORD_LENGTH);
        let mut word = 0;
        while word < HEADER_WORDS {
            (*record).header[word] = read_volatile((STREAM + 4 * word as u32) as *const u32);
            word += 1;
        }
        if (*record).stream_digest != RECORD_CRC {
            (*record).state = STATE_REFUSED_STREAM;
            (*record).tick_end = tick(record, 1);
            return;
        }
        let header = match CompressedHeader::parse(core::slice::from_raw_parts(STREAM as *const u8, 20), STREAM_EXTENT) {
            Ok(header) => header,
            Err(_) => {
                (*record).state = STATE_REFUSED_HEADER;
                (*record).tick_end = tick(record, 1);
                return;
            }
        };
        (*record).decoded_length = header.decoded_length;
        if header.decoded_length != DECODED_LENGTH || header.decoded_length > DESTINATION_CAPACITY {
            (*record).state = STATE_REFUSED_LENGTH;
            (*record).tick_end = tick(record, 1);
            return;
        }

        (*record).clock_before = clock_words();
        (*record).reservation_before = handle_word();
        (*record).resource_state_before = decode_test_stock::resource_state();
        let acquire_start = tick(record, 2);
        let reservation = match decode_test_stock::acquire() {
            Ok(reservation) => reservation,
            Err(_) => {
                (*record).acquire_ticks = tick(record, 3).wrapping_sub(acquire_start);
                (*record).reservation_acquired = handle_word();
                (*record).reservation_released = handle_word();
                (*record).resource_state_after = decode_test_stock::resource_state();
                (*record).route_after = decode_test_stock::route_request();
                (*record).clock_after = clock_words();
                (*record).refusals = (*record).refusals.wrapping_add(1);
                (*record).state = STATE_REFUSED_RESERVATION;
                (*record).tick_end = tick(record, 1);
                return;
            }
        };
        (*record).acquire_ticks = tick(record, 3).wrapping_sub(acquire_start);
        (*record).reservation_acquired = handle_word();
        (*record).clock_held = clock_word(ClockRegister::Enable);
        (*record).route_held = decode_test_stock::route_request();
        if (*record).clock_held & 4 == 0 || (*record).route_held != 8 {
            release_bookkeeping(reservation);
            (*record).reservation_released = handle_word();
            (*record).resource_state_after = decode_test_stock::resource_state();
            (*record).route_after = decode_test_stock::route_request();
            (*record).clock_after = clock_words();
            (*record).state = STATE_REFUSED_CLOCK_ROUTE;
            (*record).tick_end = tick(record, 1);
            return;
        }

        (*record).engine_before = engine_words();
        let reset_start = tick(record, 4);
        match reset_engine() {
            Some(polls) => {
                (*record).reset_polls = polls;
                (*record).reset_ticks = tick(record, 5).wrapping_sub(reset_start);
            }
            None => {
                (*record).reset_ticks = tick(record, 5).wrapping_sub(reset_start);
                release_bookkeeping(reservation);
                (*record).reservation_released = handle_word();
                (*record).resource_state_after = decode_test_stock::resource_state();
                (*record).engine_after = engine_words();
                (*record).clock_after = clock_words();
                (*record).route_after = decode_test_stock::route_request();
                (*record).refusals = (*record).refusals.wrapping_add(1);
                (*record).state = STATE_REFUSED_RESET;
                (*record).tick_end = tick(record, 1);
                return;
            }
        }

        let mut session = Session::<_, engine::ReservedClock>::open_with_clock(Probe { inner: VolatileBus, record, job: 0, triggered: false, started: false });
        let units = header.record_length.div_ceil(UNIT_BYTES);
        let mut produced = 0u32;
        let mut consumed = 0u32;
        let mut jobs = 0u32;
        let mut failure = STATE_COMPLETED;
        let mut index = 0u32;
        while index < units {
            let available = ((index + 1) * UNIT_BYTES).min(header.record_length);
            let end = match engine::feed_end(available, header.record_length, engine::PAGE_BYTES) {
                Ok(end) => end,
                Err(_) => {
                    failure = STATE_FEED_ERROR;
                    break;
                }
            };
            if consumed < end {
                if jobs as usize >= JOB_SLOTS {
                    failure = STATE_JOB_OVERFLOW;
                    break;
                }
                let slot = &mut (*record).jobs_data[jobs as usize];
                slot.unit = index;
                slot.available = available;
                slot.feed_end = end;
                slot.tick_start = tick(record, 8 + 2 * jobs as usize);
                let source = STREAM + 0x14 + consumed;
                let destination = DESTINATION + produced;
                let length = end - consumed;
                let outcome = session.submit(Source(source), Destination(destination), length);
                slot.tick_end = tick(record, 9 + 2 * jobs as usize);
                slot.destination_readback = engine_word(EngineRegister::Destination);
                match outcome {
                    Ok(count) => {
                        slot.output_count = count;
                        match produced.checked_add(count) {
                            Some(total) => {
                                produced = total;
                                slot.produced = total;
                                consumed = end;
                                jobs += 1;
                            }
                            None => {
                                jobs += 1;
                                failure = STATE_OUTPUT_OVERFLOW;
                                break;
                            }
                        }
                    }
                    Err(EngineError::Fault(_)) => {
                        jobs += 1;
                        failure = STATE_ENGINE_FAULT;
                        break;
                    }
                    Err(_) => {
                        jobs += 1;
                        failure = STATE_FEED_ERROR;
                        break;
                    }
                }
            }
            index += 1;
        }
        (*record).jobs = jobs;
        (*record).produced = produced;
        session.close();
        (*record).clock_closed = clock_word(ClockRegister::Enable);
        release_bookkeeping(reservation);
        (*record).reservation_released = handle_word();
        (*record).resource_state_after = decode_test_stock::resource_state();
        (*record).engine_after = engine_words();
        (*record).clock_after = clock_words();
        (*record).route_after = decode_test_stock::route_request();

        if failure == STATE_COMPLETED && produced != header.decoded_length {
            failure = STATE_OUTPUT_LENGTH;
        }
        let digest_start = tick(record, 6);
        (*record).output_digest = crc32_range(DESTINATION, header.decoded_length);
        (*record).image_digest = crc32_range(IMAGE4, header.decoded_length);
        let (mismatches, first, last) = compare_ranges(DESTINATION, IMAGE4, header.decoded_length);
        (*record).mismatches = mismatches;
        (*record).first_mismatch = first;
        (*record).last_mismatch = last;
        (*record).digest_ticks = tick(record, 7).wrapping_sub(digest_start);

        let output_ok = (*record).output_digest == DECODED_CRC && produced == header.decoded_length;
        let closed_engine_ok = (*record).engine_after[7] == 0
            && (*record).engine_after[0] & 1 == 1
            && (*record).engine_after[6] & engine::STATUS_CLEAR == 0;
        let clock_ok = (*record).clock_held & 4 != 0 && (*record).clock_after[1] & 4 == 0
            && (*record).clock_closed & 4 != 0
            && (*record).route_held == 8 && (*record).route_after == 0;
        let bracket_ok = (*record).reservation_acquired != 0 && (*record).reservation_released == 0;
        let resource_ok = (*record).resource_state_before == (*record).resource_state_after;
        (*record).verdict = u32::from(
            output_ok && closed_engine_ok && clock_ok && bracket_ok && resource_ok && failure == STATE_COMPLETED,
        );
        (*record).state = failure;
        (*record).tick_end = tick(record, 1);
    }
}
