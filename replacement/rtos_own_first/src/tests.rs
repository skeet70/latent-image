use super::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Write {
    at: GuestAddress,
    value: u32,
    irq_masked: bool,
}

/// Word memory that panics on an unmapped access, a CPSR whose `MSR CPSR_c` replaces bits 7:0,
/// and a log of stores with the I bit at the time of each.
struct Model {
    memory: RefCell<BTreeMap<u32, u32>>,
    cpsr: Cell<Cpsr>,
    identity: DebugIdentity,
    writes: RefCell<Vec<Write>>,
}

impl Model {
    fn new(cpsr: Cpsr) -> Self {
        Self { memory: RefCell::new(BTreeMap::new()), cpsr: Cell::new(cpsr), identity: DebugIdentity(0), writes: RefCell::new(Vec::new()) }
    }

    fn map(&self, at: GuestAddress, value: u32) {
        self.memory.borrow_mut().insert(at.0, value);
    }

    fn word(&self, at: GuestAddress) -> u32 {
        *self.memory.borrow().get(&at.0).unwrap_or_else(|| panic!("unmapped word {:#x}", at.0))
    }

    fn writes(&self) -> Vec<Write> {
        self.writes.borrow().clone()
    }
}

impl Machine for Model {
    fn load_word(&self, at: GuestAddress) -> u32 {
        assert_eq!(at.0 & 3, 0, "unaligned word load {:#x}", at.0);
        self.word(at)
    }

    fn load_byte(&self, at: GuestAddress) -> u8 {
        (self.word(GuestAddress(at.0 & !3)) >> ((at.0 & 3) * 8)) as u8
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        assert_eq!(at.0 & 3, 0, "unaligned word store {:#x}", at.0);
        self.word(at);
        self.memory.borrow_mut().insert(at.0, value);
        self.writes.borrow_mut().push(Write { at, value, irq_masked: self.cpsr.get().irq_masked() });
    }

    fn cpsr(&self) -> Cpsr {
        self.cpsr.get()
    }

    fn write_cpsr_control(&self, value: Cpsr) {
        self.cpsr.set(Cpsr((self.cpsr.get().0 & !0xff) | (value.0 & 0xff)));
    }

    fn debug_identity(&self) -> DebugIdentity {
        self.identity
    }
}

const SVC: u32 = 0x13;
const SYS: u32 = 0x1f;
const I: u32 = 0x80;
const F: u32 = 0x40;
/// N, Z, C, V, Q and all GE bits.
const FLAGS: u32 = 0xf80f_0000;
const BUFFER: GuestAddress = GuestAddress(0x1000_0000);
const SENTINEL: u32 = 0xdead_beef;
const TIMER_BASE: u32 = 0x1234_5678;
const TIME_OFFSET: u32 = 0x0fed_cba9;
const CONTEXT_WORDS: [u32; 5] = [0, 1, 2, 0x8000_0000, 0xffff_ffff];
const DOMAINS: [Domain; 2] = [Domain::A, Domain::B];

fn entry_states() -> Vec<Cpsr> {
    let mut states = Vec::new();
    for mode in [SVC, SYS] {
        for irq in [0, I] {
            for fiq in [0, F] {
                for flags in [0, FLAGS] {
                    states.push(Cpsr(flags | irq | fiq | mode));
                }
            }
        }
    }
    states
}

fn model(w: &DomainWords, cpsr: Cpsr, handler_context: u32) -> Model {
    let m = Model::new(cpsr);
    m.map(w.handler_context, handler_context);
    m.map(w.running, w.task_table.offset(59 * 56).0);
    m.map(w.dispatch_enabled, 1);
    m.map(w.timer_base, TIMER_BASE);
    m.map(w.time_offset, TIME_OFFSET);
    m.map(BUFFER, SENTINEL);
    m
}

#[derive(Clone, Copy, Debug)]
enum Service {
    GetTime,
    SetTime,
    TaskId,
    HandlerTaskId,
}

impl Service {
    fn run(self, m: &Model, w: &DomainWords) -> ServiceStatus {
        match self {
            Service::GetTime => slot_fc(m, w, BUFFER),
            Service::SetTime => slot_100(m, w, BUFFER),
            Service::TaskId => slot_128(m, w, BUFFER),
            Service::HandlerTaskId => slot_12c(m, w, BUFFER),
        }
    }

    fn admits(self, handler_context: u32) -> bool {
        match self {
            Service::HandlerTaskId => handler_context != 0,
            _ => handler_context == 0,
        }
    }

    fn write(self, w: &DomainWords) -> Write {
        match self {
            Service::GetTime => Write { at: w.time_offset, value: 0xcc79_6877, irq_masked: true },
            Service::SetTime => Write { at: BUFFER, value: 0x2222_2221, irq_masked: true },
            Service::TaskId | Service::HandlerTaskId => Write { at: BUFFER, value: 60, irq_masked: true },
        }
    }
}

#[test]
fn masked_services_run_only_in_their_context_with_irq_unmasked() {
    for w in DOMAINS.map(Domain::words) {
        for service in [Service::GetTime, Service::SetTime, Service::TaskId, Service::HandlerTaskId] {
            for context in CONTEXT_WORDS {
                for entry in entry_states() {
                    let m = model(&w, entry, context);
                    let status = service.run(&m, &w);
                    let case = format!("{service:?} {:?} context {context:#x} cpsr {:#x}", w.handler_context, entry.0);
                    if service.admits(context) && !entry.irq_masked() {
                        assert_eq!(status, ServiceStatus::OK, "{case}");
                        assert_eq!(m.writes(), vec![service.write(&w)], "{case}");
                    } else {
                        assert_eq!(status, ServiceStatus::CONTEXT_ERROR, "{case}");
                        assert_eq!(m.writes(), vec![], "{case}");
                        assert_eq!(m.word(BUFFER), SENTINEL, "{case}");
                    }
                    assert_eq!(m.cpsr(), entry, "{case}");
                }
            }
        }
    }
}

fn task_id_through(service: Service, w: &DomainWords, task: u32) -> u32 {
    let m = model(w, Cpsr(SVC), if matches!(service, Service::HandlerTaskId) { 1 } else { 0 });
    m.map(w.running, task);
    assert_eq!(service.run(&m, w), ServiceStatus::OK);
    m.word(BUFFER)
}

#[test]
fn task_ids_follow_the_stock_index_formula() {
    for w in DOMAINS.map(Domain::words) {
        let at = |bytes: i32| w.task_table.0.wrapping_add(bytes as u32);
        let cases = [
            (at(0), 1),
            (at(59 * 56), 60),
            (at(119 * 56), 120),
            (at(4), 1),
            (at(8), 0xb6db_6db8),
            (at(16), 0x6db6_db6f),
            (at(-8), 0x4924_924a),
            (at(-56), 0),
        ];
        for service in [Service::TaskId, Service::HandlerTaskId] {
            for (task, id) in cases {
                assert_eq!(task_id_through(service, &w, task), id, "{service:?} task {task:#x}");
            }
        }
    }
}

#[test]
fn null_running_task_is_zero_only_in_handler_form() {
    assert_eq!(task_id_through(Service::HandlerTaskId, &Domain::A.words(), 0), 0);
    assert_eq!(task_id_through(Service::HandlerTaskId, &Domain::B.words(), 0), 0);
    assert_eq!(task_id_through(Service::TaskId, &Domain::A.words(), 0), 0x236d_9b73);
    assert_eq!(task_id_through(Service::TaskId, &Domain::B.words(), 0), 0xfedb_292b);
}

#[test]
fn cpu_lock_services_change_only_the_i_bit() {
    type Lock = fn(&Model, &DomainWords) -> ServiceStatus;
    let services: [(&str, Lock, Service, bool); 4] = [
        ("+0x130", slot_130, Service::TaskId, true),
        ("+0x134", slot_134, Service::HandlerTaskId, true),
        ("+0x138", slot_138, Service::TaskId, false),
        ("+0x13c", slot_13c, Service::HandlerTaskId, false),
    ];
    for w in DOMAINS.map(Domain::words) {
        for (name, run, context_form, locks) in services {
            for context in CONTEXT_WORDS {
                for entry in entry_states() {
                    let m = model(&w, entry, context);
                    let status = run(&m, &w);
                    let case = format!("{name} context {context:#x} cpsr {:#x}", entry.0);
                    if context_form.admits(context) {
                        assert_eq!(status, ServiceStatus::OK, "{case}");
                        assert_eq!(m.cpsr(), entry.with_irq_masked(locks), "{case}");
                    } else {
                        assert_eq!(status, ServiceStatus::CONTEXT_ERROR, "{case}");
                        assert_eq!(m.cpsr(), entry, "{case}");
                    }
                    assert_eq!(m.writes(), vec![], "{case}");
                }
            }
        }
    }
}

#[test]
fn restore_returns_saved_i_over_the_entry_cpsr_with_flags() {
    let cases = [
        (0xf80f_00d3, 0, 0xf80f_0053),
        (0xf80f_00d3, 0x80, 0xf80f_00d3),
        (0xf80f_00d3, 0x7f, 0xf80f_0053),
        (0xf80f_0053, 0xffff_ffff, 0xf80f_00d3),
        (0x6000_001f, 0x1234_5680, 0x6000_009f),
        (0x6000_009f, 0x1234_5600, 0x6000_001f),
    ];
    for (entry, saved, result) in cases {
        let m = Model::new(Cpsr(entry));
        assert_eq!(slot_168(&m, SavedIrqMask(saved)), Cpsr(result), "entry {entry:#x} r0 {saved:#x}");
        assert_eq!(m.cpsr(), Cpsr(result), "entry {entry:#x} r0 {saved:#x}");
    }
}

#[test]
fn save_returns_prior_i_and_masks_irq() {
    for entry in entry_states() {
        let m = Model::new(entry);
        assert_eq!(slot_164(&m), SavedIrqMask(entry.0 & I), "cpsr {:#x}", entry.0);
        assert_eq!(m.cpsr(), Cpsr(entry.0 | I), "cpsr {:#x}", entry.0);
    }
}

#[test]
fn saved_mask_round_trips_through_restore() {
    for entry in entry_states() {
        let m = Model::new(entry);
        let saved = slot_164(&m);
        slot_168(&m, saved);
        assert_eq!(m.cpsr(), entry, "cpsr {:#x}", entry.0);
    }
}

#[test]
fn dispatchable_task_reads_bit2_of_byte_0xe() {
    for w in DOMAINS.map(Domain::words) {
        let task = w.task_table.offset(3 * 56);
        for (byte, flag) in [(0x00, 1), (0x04, 0), (0xfb, 1), (0xff, 0)] {
            let m = model(&w, Cpsr(SVC), 0);
            m.map(w.running, task.0);
            m.map(task.offset(0xc), 0xa5_00_5a_c3 | (byte << 16));
            assert_eq!(slot_60(&m, &w), Flag(flag), "byte {byte:#x}");
        }
        let m = model(&w, Cpsr(SVC), 0);
        m.map(w.running, 0);
        assert_eq!(slot_60(&m, &w), Flag(1));
    }
}

#[test]
fn context_queries() {
    for w in DOMAINS.map(Domain::words) {
        for context in CONTEXT_WORDS {
            for dispatch in CONTEXT_WORDS {
                for entry in entry_states() {
                    let m = model(&w, entry, context);
                    m.map(w.dispatch_enabled, dispatch);
                    let case = format!("context {context:#x} dispatch {dispatch:#x} cpsr {:#x}", entry.0);
                    assert_eq!(slot_148(&m, &w), Flag::from_bool(context != 0), "{case}");
                    assert_eq!(slot_14c(&m), Flag::from_bool(entry.0 & I != 0), "{case}");
                    assert_eq!(slot_150(&m, &w), Flag::from_bool(dispatch == 0), "{case}");
                    assert_eq!(slot_154(&m, &w), Flag::from_bool(context != 0 || entry.0 & I != 0 || dispatch == 0), "{case}");
                    assert_eq!(m.cpsr(), entry, "{case}");
                    assert_eq!(m.writes(), vec![], "{case}");
                }
            }
        }
    }
}

fn own_task_id_through_model(m: &Model, w: &DomainWords) -> TaskId {
    slot_16c(
        || slot_148(m, w),
        || (slot_128(m, w, BUFFER) == ServiceStatus::OK).then(|| TaskId(m.word(BUFFER))),
    )
}

#[test]
fn own_task_id_is_zero_in_handler_context_or_on_error() {
    for w in DOMAINS.map(Domain::words) {
        for context in CONTEXT_WORDS {
            for entry in entry_states() {
                let m = model(&w, entry, context);
                let id = own_task_id_through_model(&m, &w);
                let case = format!("context {context:#x} cpsr {:#x}", entry.0);
                let expected = if context == 0 && !entry.irq_masked() { TaskId(60) } else { TaskId(0) };
                assert_eq!(id, expected, "{case}");
                assert_eq!(m.cpsr(), entry, "{case}");
            }
        }
    }
}

#[test]
fn core_identity_is_zero_only_for_0x4003() {
    for (identity, flag) in [(0x4003, 0), (0x4002, 1), (0x1_4003, 1), (0, 1), (0xffff_ffff, 1)] {
        let mut m = Model::new(Cpsr(SVC));
        m.identity = DebugIdentity(identity);
        assert_eq!(slot_1618(&m), Flag(flag), "identity {identity:#x}");
    }
}
