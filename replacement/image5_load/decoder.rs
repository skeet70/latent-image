#![cfg_attr(target_arch = "arm", no_std)]

#[path = "../first_stage/engine.rs"]
mod engine;
mod decoder_native;
pub use decoder_native::{xm1_decoder_init, xm1_decoder_feed, xm1_decoder_progress, xm1_decoder_complete, xm1_decoder_finish};

use engine::{Bus, Destination, Framing, JobLength, Source};

#[derive(Clone, Copy)]
struct Address(u32);
#[derive(Clone, Copy)]
struct Bytes(u32);
#[derive(Clone, Copy)]
struct Task(u32);
#[derive(Clone, Copy)]
struct Handle(u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
struct ServiceStatus(i32);

impl ServiceStatus { fn succeeded(self) -> bool { self.0 == 0 } }
#[derive(Clone, Copy)]
enum Flag { Clear, Set }

impl Flag {
    const CLEAR: Self = Self::Clear;
    const SET: Self = Self::Set;
    fn is_zero(self) -> bool { matches!(self, Self::Clear) }
    fn is_one(self) -> bool { matches!(self, Self::Set) }
}

trait Word: Copy {
    fn decode(value: u32) -> Self;
    fn encode(self) -> u32;
}

macro_rules! word {
    ($($name:ident),+) => { $(impl Word for $name {
        fn decode(value: u32) -> Self { Self(value) }
        fn encode(self) -> u32 { self.0 }
    })+ };
}
word!(Address, Bytes, Task, Handle, Source, Destination);

impl Word for Flag {
    fn decode(value: u32) -> Self { if value == 0 { Self::Clear } else { Self::Set } }
    fn encode(self) -> u32 { match self { Self::Clear => 0, Self::Set => 1 } }
}

#[repr(transparent)]
struct StateCell<T> { word: core::sync::atomic::AtomicU32, marker: core::marker::PhantomData<T> }
impl<T> StateCell<T> {
    const fn zero() -> Self { Self { word: core::sync::atomic::AtomicU32::new(0), marker: core::marker::PhantomData } }
}

#[repr(C)]
struct State {
    owned: StateCell<Flag>,
    started: StateCell<Flag>,
    clock: StateCell<Handle>,
    final_started: StateCell<Flag>,
    consumed: StateCell<Bytes>,
    irq: StateCell<Handle>,
    header: StateCell<Address>,
    available: StateCell<Bytes>,
    output: StateCell<Destination>,
    input: StateCell<Source>,
    active: StateCell<Flag>,
    produced: StateCell<Bytes>,
    task: StateCell<Task>,
}

#[no_mangle]
static XM1_DECODER_STATE: State = State {
    owned: StateCell::zero(), started: StateCell::zero(), clock: StateCell::zero(),
    final_started: StateCell::zero(), consumed: StateCell::zero(), irq: StateCell::zero(),
    header: StateCell::zero(), available: StateCell::zero(), output: StateCell::zero(),
    input: StateCell::zero(), active: StateCell::zero(), produced: StateCell::zero(), task: StateCell::zero(),
};

#[derive(Clone, Copy)]
struct StateOffset(usize);
struct Cell<T> { offset: StateOffset, marker: core::marker::PhantomData<T> }
impl<T> Copy for Cell<T> {}
impl<T> Clone for Cell<T> { fn clone(&self) -> Self { *self } }
impl<T> Cell<T> {
    const fn at(offset: usize) -> Self { Self { offset: StateOffset(offset), marker: core::marker::PhantomData } }
}

const OWNED: Cell<Flag> = Cell::at(core::mem::offset_of!(State, owned));
const STARTED: Cell<Flag> = Cell::at(core::mem::offset_of!(State, started));
const CLOCK: Cell<Handle> = Cell::at(core::mem::offset_of!(State, clock));
const FINAL: Cell<Flag> = Cell::at(core::mem::offset_of!(State, final_started));
const CONSUMED: Cell<Bytes> = Cell::at(core::mem::offset_of!(State, consumed));
const IRQ: Cell<Handle> = Cell::at(core::mem::offset_of!(State, irq));
const HEADER: Cell<Address> = Cell::at(core::mem::offset_of!(State, header));
const AVAILABLE: Cell<Bytes> = Cell::at(core::mem::offset_of!(State, available));
const OUTPUT: Cell<Destination> = Cell::at(core::mem::offset_of!(State, output));
const INPUT: Cell<Source> = Cell::at(core::mem::offset_of!(State, input));
const ACTIVE: Cell<Flag> = Cell::at(core::mem::offset_of!(State, active));
const PRODUCED: Cell<Bytes> = Cell::at(core::mem::offset_of!(State, produced));
const TASK: Cell<Task> = Cell::at(core::mem::offset_of!(State, task));

#[derive(Clone, Copy)]
#[repr(u32)]
enum Resource { Engine = 0x2b, Stream = 0x48 }
#[derive(Clone, Copy)]
enum Callback { Progress, Final }
#[derive(Clone, Copy)]
#[repr(u32)]
enum RouteLevel { Released = 0, Held = 8 }
#[derive(Clone, Copy)]
#[repr(u32)]
enum Marker { Wait = 0x7819d0, FinalWait = 0x7819d8, End = 0x7819e0 }

trait Port: Bus {
    fn load<T: Word>(&mut self, cell: Cell<T>) -> T;
    fn store<T: Word>(&mut self, cell: Cell<T>, value: T);
    fn word(&mut self, address: Address) -> u32;
    fn byte(&mut self, address: Address) -> u8;
    fn task(&mut self) -> Task;
    fn take(&mut self, resource: Resource) -> ServiceStatus;
    fn give(&mut self, resource: Resource) -> ServiceStatus;
    fn route(&mut self, level: RouteLevel);
    fn reserve_clock(&mut self);
    fn release_clock(&mut self, handle: Handle);
    fn attach(&mut self, callback: Callback);
    fn detach(&mut self, handle: Handle);
    fn marker(&mut self, marker: Marker);
}

#[derive(Debug)]
enum Refusal { Acquire, Release, Uninitialized, ZeroPage }

struct Decoder<P: Port> { port: P }
impl<P: Port> Decoder<P> {
    fn new(port: P) -> Self { Self { port } }

    fn ready(&mut self) -> Result<(), Refusal> {
        if self.port.load(OWNED).is_one() && self.port.load(STARTED).is_one() { Ok(()) }
        else { Err(Refusal::Uninitialized) }
    }

    fn start(&mut self) -> Result<(), Refusal> {
        if self.port.load(OWNED).is_zero() {
            if !self.port.take(Resource::Engine).succeeded() { return Err(Refusal::Acquire); }
            self.port.store(OWNED, Flag::SET);
        }
        self.port.route(RouteLevel::Held);
        self.port.reserve_clock();
        engine::reset(&mut self.port);
        self.port.store(STARTED, Flag::SET);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Refusal> {
        if self.port.load(OWNED).is_one() {
            self.port.store(OWNED, Flag::CLEAR);
            self.port.store(STARTED, Flag::CLEAR);
            engine::close(&mut self.port);
            self.port.route(RouteLevel::Released);
            let handle = self.port.load(CLOCK);
            self.port.release_clock(handle);
            self.port.store(CLOCK, Handle(0));
            if !self.port.give(Resource::Engine).succeeded() { return Err(Refusal::Release); }
        }
        Ok(())
    }

    fn program(&mut self, source: Source, destination: Destination, length: Bytes, framing: Framing) -> Result<(), Refusal> {
        self.ready()?;
        engine::program(&mut self.port, source, destination, JobLength::from_register_field(length.0), framing);
        Ok(())
    }

    fn kick(&mut self) -> Result<(), Refusal> {
        self.ready()?;
        engine::trigger(&mut self.port);
        Ok(())
    }

    fn produced(&mut self) -> Result<Bytes, Refusal> {
        self.ready()?;
        Ok(Bytes(engine::take_output(&mut self.port)))
    }

    fn length(&mut self) -> Bytes {
        let header = self.port.load(HEADER);
        Bytes(self.port.word(Address(header.0.wrapping_add(4))))
    }

    fn ready_length(&mut self, header: Address, available: Bytes) -> Result<Bytes, Refusal> {
        let length = self.port.word(Address(header.0.wrapping_add(4)));
        if length <= available.0 { return Ok(Bytes(length.wrapping_sub(20))); }
        let page = self.port.word(Address(header.0.wrapping_add(12)));
        if page == 0 { return Err(Refusal::ZeroPage); }
        Ok(Bytes((available.0 / page * page).wrapping_sub(20)))
    }

    fn submit_tail(&mut self, length: Bytes) -> Result<(), Refusal> {
        let source = self.port.load(INPUT);
        let consumed = self.port.load(CONSUMED);
        let output = self.port.load(OUTPUT);
        let produced = self.port.load(PRODUCED);
        let header = self.port.load(HEADER);
        let framing = Framing::from_header_mode(self.port.byte(Address(header.0.wrapping_add(16))));
        self.program(Source(source.0.wrapping_add(consumed.0)), Destination(output.0.wrapping_add(produced.0)), length, framing)
    }

    fn init(&mut self, destination: Destination, staging: Source) -> Result<(), Refusal> {
        let staging = staging.0 | 0x40000000;
        self.port.store(HEADER, Address(staging));
        self.port.store(INPUT, Source(staging.wrapping_add(20)));
        self.port.store(OUTPUT, Destination(destination.0 | 0x40000000));
        self.port.store(PRODUCED, Bytes(0));
        self.port.store(AVAILABLE, Bytes(0));
        self.port.store(CONSUMED, Bytes(0));
        self.port.store(FINAL, Flag::CLEAR);
        self.port.store(ACTIVE, Flag::CLEAR);
        let task = self.port.task();
        self.port.store(TASK, task);
        let _ = self.port.take(Resource::Stream);
        self.start()
    }

    fn feed(&mut self, loaded: Bytes) -> Result<(), Refusal> {
        if self.port.load(AVAILABLE).0 > loaded.0 || loaded.0 <= 20 || !self.port.load(FINAL).is_zero() { return Ok(()); }
        let total = self.length();
        let loaded = Bytes(loaded.0.min(total.0));
        self.port.store(AVAILABLE, loaded);
        if !self.port.load(ACTIVE).is_zero() { return Ok(()); }
        let header = self.port.load(HEADER);
        let ready = self.ready_length(header, loaded)?;
        let consumed = self.port.load(CONSUMED);
        if ready.0 <= consumed.0 { return Ok(()); }
        self.port.store(ACTIVE, Flag::SET);
        if total.0 == loaded.0 {
            self.port.store(FINAL, Flag::SET);
            self.port.attach(Callback::Final);
        } else { self.port.attach(Callback::Progress); }
        self.submit_tail(Bytes(ready.0.wrapping_sub(consumed.0)))?;
        self.port.store(CONSUMED, ready);
        self.kick()
    }

    fn account(&mut self) -> Result<(), Refusal> {
        let count = self.produced()?;
        let produced = self.port.load(PRODUCED);
        self.port.store(PRODUCED, Bytes(produced.0.wrapping_add(count.0)));
        let handle = self.port.load(IRQ);
        self.port.detach(handle);
        self.port.store(IRQ, Handle(0));
        Ok(())
    }

    fn progress(&mut self) -> Result<(), Refusal> {
        self.account()?;
        let available = self.port.load(AVAILABLE);
        let consumed = self.port.load(CONSUMED);
        let mut active = Flag::CLEAR;
        if consumed.0 < available.0.wrapping_sub(20) {
            let header = self.port.load(HEADER);
            let ready = self.ready_length(header, available)?;
            if consumed.0 < ready.0 {
                let total = self.length();
                if available.0 == total.0 {
                    self.port.store(FINAL, Flag::SET);
                    self.port.attach(Callback::Final);
                } else { self.port.attach(Callback::Progress); }
                self.submit_tail(Bytes(ready.0.wrapping_sub(consumed.0)))?;
                self.port.store(CONSUMED, ready);
                self.kick()?;
                active = Flag::SET;
            }
        }
        self.port.store(ACTIVE, active);
        Ok(())
    }

    fn complete(&mut self) -> Result<(), Refusal> {
        self.account()?;
        if self.port.load(TASK).0 != 0 { let _ = self.port.give(Resource::Stream); }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), Refusal> {
        let total = self.length();
        self.port.store(AVAILABLE, total);
        if !self.port.load(ACTIVE).is_zero() {
            self.port.marker(Marker::Wait);
            let _ = self.port.take(Resource::Stream);
        }
        if self.port.load(FINAL).is_zero() {
            self.port.attach(Callback::Final);
            let total = self.length();
            let consumed = self.port.load(CONSUMED);
            self.submit_tail(Bytes(total.0.wrapping_sub(consumed.0).wrapping_sub(20)))?;
            self.kick()?;
            self.port.marker(Marker::FinalWait);
            let _ = self.port.take(Resource::Stream);
        }
        self.stop()?;
        let _ = self.port.give(Resource::Stream);
        self.port.marker(Marker::End);
        Ok(())
    }
}
