//! A stock machine with byte memory that records each entry it is asked to enter, the
//! profile-record entries other tasks share, and peer ports over the kernel's fake.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use xm1_kernel::fake::{Call, Fake as KernelFake};
use xm1_kernel::{Form, Kernel, TaskContext, TaskId};
use xm1_saclac_interface::stock::record_handle_from_a2;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::layout::{slot_grant, slot_record, slot_serial, GRANT_READ, GRANT_WRITE};
use xm1_saclac_profile_records::stock::record_handle;
use xm1_saclac_profile_records::{
    AccessGroup, Bytes, Grants, Handle, MeasurementSelection, OpenMode, RecordImage, Scope, Serial, SharedHeaderByte, Slot,
    StockEntries, Tag,
};
use xm1_saclac_peers::{EmulatorOnly, PeerPorts};
use xm1_saclac_shared::stock::{Argument, Machine};

use crate::Services;

/// Where the fake keeps the adjustment area 0x69f8fc reads; outside every address the tests use.
const ADJUSTMENTS: u32 = 0xa000_0000;
const ADJUSTMENT_BYTE: u32 = 0x69_f8fc;

#[derive(Default)]
pub struct State {
    memory: HashMap<u32, u8>,
    answers: HashMap<u32, VecDeque<u32>>,
    entered: Vec<(u32, Vec<u32>)>,
    buffer_writes: HashMap<u32, Vec<(usize, Vec<u8>)>>,
}

impl State {
    fn byte(&self, address: u32) -> u8 { self.memory.get(&address).copied().unwrap_or(0) }

    fn write(&mut self, address: u32, value: u32, bytes: u32) {
        for offset in 0..bytes {
            self.memory.insert(address + offset, (value >> (8 * offset)) as u8);
        }
    }
}

#[derive(Clone, Default)]
pub struct Fake(Rc<RefCell<State>>);

impl Fake {
    pub fn set_word(&self, address: u32, value: u32) { self.0.borrow_mut().write(address, value, 4) }

    pub fn set_byte(&self, address: u32, value: u8) { self.0.borrow_mut().memory.insert(address, value); }

    pub fn byte(&self, address: u32) -> u8 { self.0.borrow().byte(address) }

    pub fn word(&self, address: u32) -> u32 { self.read_u32(address) }

    pub fn answer(&self, address: u32, value: u32) { self.0.borrow_mut().answers.entry(address).or_default().push_back(value) }

    /// Sets the byte 0x69f8fc answers for `key`.
    pub fn adjustment(&self, key: u32, value: u8) { self.set_byte(ADJUSTMENTS + key, value) }

    pub fn take_entered(&self) -> Vec<(u32, Vec<u32>)> { std::mem::take(&mut self.0.borrow_mut().entered) }

    /// A retained entry's writes to its caller's buffer.
    pub fn buffer(&self, address: u32, offset: usize, bytes: &[u8]) {
        self.0.borrow_mut().buffer_writes.entry(address).or_default().push((offset, bytes.to_vec()));
    }
}

impl Machine for Fake {
    fn read_u16(&self, address: u32) -> u16 {
        let state = self.0.borrow();
        u16::from_le_bytes([state.byte(address), state.byte(address + 1)])
    }

    fn read_u32(&self, address: u32) -> u32 {
        let state = self.0.borrow();
        u32::from_le_bytes([0, 1, 2, 3].map(|offset| state.byte(address + offset)))
    }

    fn write_u8(&mut self, address: u32, value: u8) { self.set_byte(address, value) }

    fn write_u32(&mut self, address: u32, value: u32) { self.set_word(address, value) }

    fn enter(&self, address: u32, arguments: &[usize]) -> u32 {
        let mut state = self.0.borrow_mut();
        if address == ADJUSTMENT_BYTE {
            return u32::from(state.byte(ADJUSTMENTS + arguments[0] as u32));
        }
        state.entered.push((address, arguments.iter().map(|&argument| argument as u32).collect()));
        state.answers.get_mut(&address).and_then(VecDeque::pop_front).unwrap_or(0)
    }

    fn enter_with(&self, address: u32, arguments: &[Argument], buffer: &mut [u8]) -> u32 {
        let arguments: Vec<_> = arguments.iter().map(|argument| match *argument {
            Argument::Word(word) => word as usize,
            Argument::Buffer(offset) => 0xb000_0000 + offset,
        }).collect();
        let answer = self.enter(address, &arguments);
        if let Some(writes) = self.0.borrow().buffer_writes.get(&address) {
            for (offset, bytes) in writes { buffer[*offset..*offset + bytes.len()].copy_from_slice(bytes); }
        }
        answer
    }
}

/// The shared profile-record entries, answered from fixed values.
#[derive(Clone, Default)]
pub struct Entries {
    pub selection: MeasurementSelection,
    pub header_47: u8,
    pub header_31: u8,
    pub header_63: u8,
}

impl StockEntries for Entries {
    fn measurement_selection(&mut self, _: RecordHandle) -> MeasurementSelection { self.selection }

    fn header_byte(&mut self, _: RecordHandle, byte: SharedHeaderByte) -> u8 {
        match byte {
            SharedHeaderByte::At47 => self.header_47,
            SharedHeaderByte::At31 => self.header_31,
            SharedHeaderByte::At63 => self.header_63,
            _ => 0,
        }
    }

    fn lens_designation(&mut self, _: RecordHandle) -> [u8; 28] { [0; 28] }

    fn replace_header(&mut self, _: RecordHandle, _: &RecordImage) {}

    fn open(&mut self, _: OpenMode, _: Grants) -> RecordHandle { record_handle_from_a2(u32::MAX) }

    fn close(&mut self, _: RecordHandle) {}

    fn initialize(&mut self) {}
}

pub type Root<P = ()> = Services<Fake, Bytes, Entries, P>;

/// The root with no peer ports, for bindings that post nothing.
pub fn root(bytes: Bytes, entries: Entries) -> (Fake, Root) { root_with((), bytes, entries) }

pub fn root_with<P>(peers: P, bytes: Bytes, entries: Entries) -> (Fake, Root<P>) {
    let fake = Fake::default();
    (fake.clone(), Services::new(fake, bytes, entries, peers))
}

static LENS_SAFETY_MARKER: u32 = 1;

/// The peer ports a task-43 context posts through, lens-reaching requests included.
pub type Peers<'c, 'k> = PeerPorts<'c, TaskContext<'k, KernelFake>, EmulatorOnly>;

/// A kernel whose running task is saclac's, task 43.
pub fn saclac_kernel() -> Kernel<KernelFake> { Kernel::new(KernelFake::with_task(TaskId::new(43).unwrap())) }

pub fn peers<'c, 'k>(cx: &'c TaskContext<'k, KernelFake>) -> Peers<'c, 'k> {
    PeerPorts::new(cx, EmulatorOnly::new(&LENS_SAFETY_MARKER).unwrap())
}

/// Each post as (queue, operation, payload), asserting the task form, sender 43 and the queue as
/// destination.
pub fn posts(kernel: &Kernel<KernelFake>) -> Vec<(u8, u16, Vec<u8>)> {
    kernel
        .backend()
        .calls()
        .into_iter()
        .filter_map(|call| match call {
            Call::Post { id, form, header, payload } => {
                assert_eq!((form, header.sender, header.destination), (Form::Task, 43, id.get()), "{header:?}");
                Some((id.get(), header.operation, payload))
            }
            _ => None,
        })
        .collect()
}

/// Opens slot 0 on the first record with read and write grants for `groups`, and returns its
/// owner handle.
pub fn open(bytes: &mut Bytes, groups: &[u8]) -> RecordHandle {
    let slot = Slot::new(0).unwrap();
    let serial = Serial::new(1).unwrap();
    bytes.registry[usize::from(slot_serial(slot))..][..4].copy_from_slice(&1u32.to_le_bytes());
    bytes.registry[usize::from(slot_record(slot))..][..4].copy_from_slice(&0u32.to_le_bytes());
    for &group in groups {
        let group = AccessGroup::new(group).unwrap();
        bytes.registry[usize::from(slot_grant(slot, group))] = GRANT_READ | GRANT_WRITE;
    }
    record_handle(Handle { slot, serial, owner: true, writable: true, scope: Scope::ALL, tag: Tag(0) })
}
