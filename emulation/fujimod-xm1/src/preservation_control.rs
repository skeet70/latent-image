//! Synthetic native evidence controls; no camera image or installed ownership claim.
use fujimod_emulator::board::{Board, bus::{Bus, Mode}, memory::{Memory, BusMapping}};
use fujimod_emulator_cpu::{*, memory::Backing, preservation::{*, shadow}};
use fujimod_emulator_cpu_unicorn::{Library, rprofile};
use fujimod_emulator_devices::{Device, Cx};
use fujimod_xm1::{
    modal_lifetime::LifetimePhase,
    modal_permit::{CoreId as ModalCoreId, Count as PublicationCount, Cpsr as ModalCpsr},
    modal_task::{self, KernelId, MemoryError, StackExtent, TaskExecution, TaskIdentity,
        ValidatedTaskMemory},
    provenance::Guest,
};
use serde_json::{Value, json};
use std::{cell::{Cell, RefCell}, ptr::NonNull, rc::Rc};

const DEVICE: u64 = 0x80000000;
#[derive(serde::Serialize, serde::Deserialize)]
struct Pending { due: Option<u64>, busy: bool, completed: u64 }
impl Device for Pending {
    fn name(&self) -> &'static str { "synthetic-preservation-pending-work" }
    fn fingerprint(&self) -> Result<Value, String> { serde_json::to_value(self).map_err(|e| e.to_string()) }
    fn read(&mut self, _: &mut Cx<'_>, _: u64, _: u8) -> u64 { self.completed }
    fn write(&mut self, cx: &mut Cx<'_>, _: u64, _: u8, _: u64) { self.busy = true; cx.schedule(3000, 7); }
    fn next_deadline(&self, _: u64) -> Option<u64> { self.due }
    fn advance(&mut self, cx: &mut Cx<'_>) {
        if self.due.is_some_and(|due| due <= cx.now()) { self.due = None; }
    }
    fn event(&mut self, cx: &mut Cx<'_>, token: u64) {
        if token != 7 || !self.busy { cx.fail("unexpected synthetic pending-work completion"); return; }
        self.busy = false; self.completed += 1;
    }
}

fn set(cpu: &dyn Cpu, register: Register, value: u32) -> Result<(), String> { cpu.write_register(register, &value.to_le_bytes()) }
fn get(cpu: &dyn Cpu, register: Register) -> Result<u32, String> {
    let mut bytes = [0; 4];
    if cpu.read_register(register, &mut bytes)? != 4 { return Err("short A32 register".into()); }
    Ok(u32::from_le_bytes(bytes))
}
fn registers(board: &Board) -> Result<[u32; 16], String> {
    let cpu = board.cpu(0)?;
    let mut values = [0; 16];
    for (index, value) in values.iter_mut().enumerate() { *value = get(cpu, arm::CORE[index])?; }
    Ok(values)
}
fn words(board: &Board, address: u64, values: &[u32]) -> Result<(), String> {
    board.write_physical(address, &values.iter().flat_map(|word| word.to_le_bytes()).collect::<Vec<_>>())
}
fn board(worker: bool, restart_alias: bool) -> Result<Board, String> {
    let cpu: Box<dyn Cpu> = if worker {
        Box::new(rprofile::worker::spawn(rprofile::worker::Config { translation_cache_bytes: 1 << 20,
            options: Default::default(), memory: rprofile::memory::SharedMemory::default() })?)
    } else {
        Box::new(rprofile::core::Core::new(Library::linked()?.open(Model::CortexR5F, 1 << 20)?,
            Default::default(), rprofile::core::Wiring { memory: rprofile::memory::Memory::default() })?)
    };
    let backing = Backing::aligned_zeros(0x8000, 0x4000)?;
    unsafe { cpu.map_physical(0, 0, 0x8000, 7, NonNull::new(backing.as_ptr()).ok_or("null backing")?) }?;
    if restart_alias {
        unsafe { cpu.map_physical(0x8000, 0x1000, 0x1000, 7,
            NonNull::new(backing.as_ptr().add(0x1000)).ok_or("null alias backing")?) }?;
    }
    set(&*cpu, arm::CPSR, 0x13)?; set(&*cpu, arm::SPSR, 0x53)?;
    set(&*cpu, arm::CPSR, 0x12)?; set(&*cpu, arm::CORE[13], 0xd3)?; set(&*cpu, arm::SPSR, 0x53)?;
    set(&*cpu, arm::CPSR, 0x80000153)?;
    for index in 0..13 { set(&*cpu, arm::CORE[index], 0x10000000 + index as u32)?; }
    set(&*cpu, arm::CORE[0], 1)?; set(&*cpu, arm::CORE[1], DEVICE as u32)?;
    set(&*cpu, arm::CORE[13], 0x7000)?; set(&*cpu, arm::CORE[14], 0x1800)?; set(&*cpu, arm::PC, 0xffc)?;
    let memory = Memory::new(vec![(0, backing)], vec![BusMapping::ram(0, 0x8000)])?;
    let mut bus = Bus::new(Mode::Strict);
    bus.register(DEVICE, 4, Box::new(Pending { due: Some(2000), busy: false, completed: 0 }))?;
    let mut board = Board::new(vec![cpu], bus, timeline::Ratio::new(1, 1)?, None, vec![true], memory)?;
    board.set_execution_policy(ExecutionPolicy::Exact)?;
    board.map_mmio(0, DEVICE, 0x1000, DEVICE)?;
    words(&board, 0x18, &[0xea0007f8])?;
    words(&board, 0xffc, &[0xe5810000, 0xe92d4ff0, 0xe320f003])?;
    // Standard ARM encodings construct the witnessed bank transfer/return shape.
    // The middle ISR callback body is absent: this is not a firmware qualification.
    words(&board, 0x2000, &[0xe3a0d0d3, 0xe129f00d, 0xe92dd00f, 0xe3a000d2, 0xe129f000,
        0xe24e0004, 0xe14f1000, 0xe2012040, 0xe3822093, 0xe129f002, 0xe58d0018,
        0xe92d0002, 0xe8bd0002, 0xe10f2000, 0xe2022040, 0xe3c11040, 0xe1811002,
        0xe169f001, 0xe8fdd00f])?;
    Ok(board)
}

struct Trace {
    binding: shadow::Binding<(u64, u64)>, stock: shadow::StockReturn,
    events: Vec<PreservationEvidence>, registers: [u32; 16], cpsr: u32, frame: SavedFrame,
    pending: Value, finished: Value,
}
fn execute(worker: bool, context_adapter: bool, enabled: bool, injection: &str) -> Result<Trace, String> {
    let mut board = board(worker, false)?;
    let events = Rc::new(RefCell::new(Vec::with_capacity(32)));
    let observed = events.clone();
    let callback = Rc::new(move |_: fujimod_emulator::board::observation::CoreIndex, event| {
        let mut events = observed.borrow_mut();
        assert!(events.len() < events.capacity(), "synthetic event capacity exhausted");
        events.push(event);
    });
    let range = HookRange::Inclusive { begin: 0x1000, end: 0x1000 };
    let mut watch = if !enabled { None } else if context_adapter {
        Some(board.observation_context().preservation_evidence_begin(0, range, callback)?)
    } else { Some(board.preservation_evidence_begin(0, range, callback)?) };
    if worker && enabled {
        let before = board.fingerprint()?;
        if board.cpu(0)?.set_execution_policy(ExecutionPolicy::Interactive).is_ok()
            || board.fingerprint()? != before {
            return Err("worker evidence allowed an unsafe policy transition or changed guest state".into());
        }
    }
    let status_injection = matches!(injection, "saved-status-before-write" | "wrong-bank-before-write");
    let addresses = if status_injection { vec![0x1000, 0x1004, 0x2044] } else { vec![0x1000, 0x1004] };
    let mut points = board.instruction_points_begin(0, addresses)?;
    board.run_until(2, 1)?;
    if points.event() != Some(0x1000) { return Err("missing actual pre-PUSH point".into()); }
    if !events.borrow().is_empty() { return Err("unexecuted PUSH produced native evidence".into()); }
    let entry = points.event_context().ok_or("missing pre-PUSH identity")?.entry;
    let binding = shadow::Binding { entry, registers: registers(&board)?, cpsr: get(board.cpu(0)?, arm::CPSR)?, context: (1, 1) };
    points.resume(0x1000, 0x1000);
    if injection == "a-before-retirement" { set(board.cpu(0)?, arm::CPSR, binding.cpsr ^ 0x100)?; }
    board.run_until(2, 1)?;
    if injection == "a-before-irq" { set(board.cpu(0)?, arm::CPSR, binding.cpsr ^ 0x100)?; }
    if injection != "a-before-retirement" {
        board.cpu(0)?.set_interrupt(Interrupt::Irq, true)?;
        board.run_until(3, 1)?;
        board.cpu(0)?.set_interrupt(Interrupt::Irq, false)?;
    }
    board.run_until(100, 1)?;
    if status_injection {
        if points.event() != Some(0x2044) { return Err("missing actual saved-status writer point".into()); }
        if injection == "saved-status-before-write" {
            let saved = get(board.cpu(0)?, arm::CORE[1])?;
            set(board.cpu(0)?, arm::CORE[1], saved ^ 0x100)?;
        } else {
            let sp = get(board.cpu(0)?, arm::CORE[13])?;
            let cpsr = get(board.cpu(0)?, arm::CPSR)?;
            set(board.cpu(0)?, arm::CPSR, (cpsr & !0x1f) | 0x12)?;
            set(board.cpu(0)?, arm::CORE[13], sp)?;
        }
        points.resume(0x2044, 0x2044);
        board.run_until(100, 1)?;
    }
    if points.event() != Some(0x1004) { return Err("missing actual late destination point".into()); }
    let mut late_cpsr = get(board.cpu(0)?, arm::CPSR)?;
    if injection == "a-after-return" { late_cpsr ^= 0x100; set(board.cpu(0)?, arm::CPSR, late_cpsr)?; }
    if injection == "flags-after-return" { late_cpsr ^= 0x80000000; set(board.cpu(0)?, arm::CPSR, late_cpsr)?; }
    let late_registers = registers(&board)?;
    let mut saved = [0; 36]; board.cpu(0)?.read_memory(0x6fdc, &mut saved)?;
    let frame = SavedFrame { address: 0x6fdc, physical: 0x6fdc,
        words: std::array::from_fn(|index| u32::from_le_bytes(saved[index*4..index*4+4].try_into().unwrap())) };
    let work = board.inspect_device::<Pending, _>(0, |device| Ok((device.due, device.busy, device.completed)))?;
    if work != (Some(2000), true, 0) { return Err(format!("pending timer/queued work was lost: {work:?}")); }
    let pending = board.fingerprint()?;
    if let Some(watch) = &mut watch { watch.remove()?; }
    board.instruction_points_end(0, &mut points)?;
    board.run_until(3001, 1)?;
    let work = board.inspect_device::<Pending, _>(0, |device| Ok((device.due, device.busy, device.completed)))?;
    if work != (None, false, 1) { return Err(format!("queued work did not complete after removal: {work:?}")); }
    let finished = board.fingerprint()?;
    let source = |address, word| ExecutionSource { address, physical: u64::from(address), word, tcm: entry.tcm };
    let stock = shadow::StockReturn { vector: 0x18, status: source(0x2044, 0xe169f001), returning: source(0x2048, 0xe8fdd00f) };
    let events = events.borrow().clone();
    Ok(Trace { binding, stock, events, registers: late_registers, cpsr: late_cpsr, frame, pending, finished })
}
fn judge_bound<C: Copy + Eq>(binding: shadow::Binding<C>, stock: shadow::StockReturn,
    events: &[PreservationEvidence], registers: [u32; 16], cpsr: u32, context: C,
    frame: SavedFrame) -> Result<(), shadow::Failure> {
    let mut verifier = shadow::Shadow::new(binding, stock)?;
    for event in events { verifier.observe(*event)?; }
    verifier.handoff(registers, cpsr, frame, context)?;
    verifier.finish()
}
fn judge(trace: &Trace, events: &[PreservationEvidence], context: (u64, u64), frame: SavedFrame) -> Result<(), shadow::Failure> {
    judge_bound(trace.binding, trace.stock, events, trace.registers, trace.cpsr, context, frame)
}
fn refused(trace: &Trace, events: &[PreservationEvidence], context: (u64, u64), frame: SavedFrame) -> Result<Value, String> {
    let failure = judge(trace, events, context, frame).err().ok_or("negative control was admitted")?;
    Ok(json!({"refused":true,"failure":format!("{failure:?}")}))
}
fn faulted_native() -> Result<Value, String> {
    let mut board = board(false, false)?;
    let events = Rc::new(RefCell::new(Vec::with_capacity(8)));
    let observed = events.clone();
    let callback = Rc::new(move |_, event| {
        let mut events = observed.borrow_mut();
        assert!(events.len() < events.capacity());
        events.push(event);
    });
    let mut watch = board.preservation_evidence_begin(0,
        HookRange::Inclusive { begin: 0x1000, end: 0x1000 }, callback)?;
    let mut points = board.instruction_points_begin(0, vec![0x1000])?;
    board.run_until(2, 1)?;
    if points.event() != Some(0x1000) || !events.borrow().is_empty() {
        return Err("fault control did not reach an unexecuted PUSH".into());
    }
    set(board.cpu(0)?, arm::CORE[13], 0x10)?;
    points.resume(0x1000, 0x1000);
    let outcome = board.run_until(2, 1);
    watch.remove()?;
    board.instruction_points_end(0, &mut points)?;
    let events = events.borrow();
    if events.iter().any(|event| matches!(event, PreservationEvidence::Retired(_)))
        || !events.iter().any(|event| matches!(event, PreservationEvidence::Refused { reason: EvidenceFailure::Faulted, .. })) {
        return Err("faulted native PUSH lacks its refusal or fabricated retirement".into());
    }
    Ok(json!({"refused":true,"native_fault":true,"run_error":outcome.err(),
        "events":events.iter().map(|event|format!("{event:?}")).collect::<Vec<_>>()}))
}

fn partial_store(worker: bool, context_adapter: bool) -> Result<Value, String> {
    let mut board = board(worker, false)?;
    set(board.cpu(0)?, arm::CORE[13], 0x800c)?;
    let events = Rc::new(RefCell::new(Vec::with_capacity(16)));
    let observed = events.clone();
    let callback = Rc::new(move |_, event| {
        eprintln!("partial-store native {event:?}");
        let mut events = observed.borrow_mut();
        assert!(events.len() < events.capacity());
        events.push(event);
    });
    let range = HookRange::Inclusive { begin: 0x1000, end: 0x1000 };
    let mut watch = if context_adapter {
        board.observation_context().preservation_evidence_begin(0, range, callback)?
    } else { board.preservation_evidence_begin(0, range, callback)? };
    let mut points = board.instruction_points_begin(0, vec![0x1000])?;
    board.run_until(2, 1)?;
    if points.event() != Some(0x1000) || !events.borrow().is_empty() {
        return Err("partial-store control emitted early evidence".into());
    }
    let mut before = [0; 24];
    board.cpu(0)?.read_memory(0x7fe8, &mut before)?;
    points.resume(0x1000, 0x1000);
    let run_error = board.cpu(0)?.run(Run { begin: 0x1000, until: None, watchdog_us: 0, instructions: Some(1) }).err();
    let mut after = [0; 24];
    board.cpu(0)?.read_memory(0x7fe8, &mut after)?;
    let evidence = events.borrow().clone();
    let prefix: [u32; 6] = std::array::from_fn(|index| u32::from_le_bytes(after[index*4..index*4+4].try_into().unwrap()));
    let passed = run_error.is_some() && before != after
        && prefix == std::array::from_fn(|index| 0x10000004 + index as u32)
        && !evidence.iter().any(|e| matches!(e, PreservationEvidence::Retired(_)))
        && evidence.iter().any(|e| matches!(e, PreservationEvidence::Refused { reason: EvidenceFailure::Faulted, .. }));
    watch.remove()?;
    board.instruction_points_end(0, &mut points)?;
    Ok(json!({"case":"partial-store","worker":worker,"context_adapter":context_adapter,"passed":passed,
        "run_error":run_error.map(|e|e.to_string()),"before":before,"after":after,"committed_prefix":prefix,
        "events":evidence.iter().map(|e|format!("{e:?}")).collect::<Vec<_>>()}))
}

fn bank_controls() -> Result<Value, String> {
    let trace = execute(false, false, true, "")?;
    judge(&trace, &trace.events, trace.binding.context, trace.frame).map_err(|e|format!("bank positive: {e:?}"))?;
    let mut controls = serde_json::Map::new();
    for bank in ["irq_sp", "irq_lr", "svc_sp", "svc_lr"] {
        let mut events = trace.events.clone();
        for event in &mut events {
            let states = match event {
                PreservationEvidence::SavedStatus(write) => Some((&mut write.before, &mut write.after)),
                PreservationEvidence::Returned(returned) => Some((&mut returned.before, &mut returned.after)),
                _ => None,
            };
            if let Some((before, after)) = states {
                for state in [before, after] {
                    match bank {
                        "irq_sp" => state.irq_sp ^= 0x100,
                        "irq_lr" => state.irq_lr ^= 0x100,
                        "svc_sp" => state.svc_sp ^= 0x100,
                        "svc_lr" => state.svc_lr ^= 0x100,
                        _ => unreachable!(),
                    }
                }
            }
        }
        let failure = judge(&trace, &events, trace.binding.context, trace.frame).err();
        controls.insert(bank.into(), json!({"refused":failure.is_some(),"failure":failure.map(|e|format!("{e:?}")),
            "events":events.iter().map(|e|format!("{e:?}")).collect::<Vec<_>>()}));
    }
    let passed = controls.values().all(|control|control["refused"] == true);
    Ok(json!({"case":"independent-banks","passed":passed,"controls":controls,
        "original_events":trace.events.iter().map(|e|format!("{e:?}")).collect::<Vec<_>>()}))
}

const ACTUAL_CREATION_WORDS: [u32; 10] =
    [2, 0, 12726112, 5, 3072, 10797568, 0, 0, 1, 1];

struct CopiedTaskMemory {
    context: (Guest, [u8; 4]),
    current: (Guest, [u8; 4]),
    backlink: (Guest, [u8; 4]),
    creation: (Guest, [u8; 40]),
    stack_start: Guest,
    stack_end: Guest,
}
impl ValidatedTaskMemory for CopiedTaskMemory {
    fn read(&self, address: Guest, output: &mut [u8]) -> Result<(), MemoryError> {
        let source: &[u8] = if address == self.context.0 && output.len() == self.context.1.len() {
            &self.context.1
        } else if address == self.current.0 && output.len() == self.current.1.len() {
            &self.current.1
        } else if address == self.backlink.0 && output.len() == self.backlink.1.len() {
            &self.backlink.1
        } else if address == self.creation.0 && output.len() == self.creation.1.len() {
            &self.creation.1
        } else {
            return Err(MemoryError::Unmapped);
        };
        output.copy_from_slice(source);
        Ok(())
    }

    fn validate_stack(&self, extent: StackExtent) -> Result<(), MemoryError> {
        let physical = extent.physical();
        if physical.start() == self.stack_start && physical.end() == self.stack_end {
            Ok(())
        } else {
            Err(MemoryError::PhysicalMismatch)
        }
    }
}

fn copied_task(kernel: KernelId, creation_words: [u32; 10]) -> Result<TaskExecution, String> {
    let layout = modal_task::source_layouts().into_iter().find(|layout| layout.kernel == kernel)
        .ok_or("canonical task layout is absent")?;
    let node = Guest(layout.nodes.start().0 | 0x40000000);
    let creation = Guest(layout.creations.start().0 | 0x40000000);
    if creation_words[8] != 1 {
        return Err("copied canonical task fixture is not task 1".into());
    }
    let mut creation_bytes = [0; 40];
    for (index, word) in creation_words.into_iter().enumerate() {
        creation_bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    let stack_start = Guest(creation_words[5] & !0x40000000);
    let stack_end = Guest(stack_start.0.checked_add(creation_words[4])
        .ok_or("task stack address overflow")?);
    let memory = CopiedTaskMemory {
        context: (kernel.context_cell(), 0u32.to_le_bytes()),
        current: (kernel.current_cell(), node.0.to_le_bytes()),
        backlink: (Guest(node.0 + 8), creation.0.to_le_bytes()),
        creation: (creation, creation_bytes),
        stack_start,
        stack_end,
    };
    let core = match kernel { KernelId::Primary => ModalCoreId(0), KernelId::Secondary => ModalCoreId(1) };
    let task = modal_task::sample(&memory, core, Guest(10800568), ModalCpsr(0x80000153))
        .map_err(|error| format!("copied canonical task fixture refused: {error:?}"))?;
    let identity = task.identity();
    if identity.node.address() != node || identity.creation.address() != creation {
        return Err("canonical task fixture changed copied identity".into());
    }
    Ok(task)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActualContext {
    task: TaskExecution,
    phase: LifetimePhase,
    forward: PublicationCount,
    reverse: PublicationCount,
    install_core: Option<ModalCoreId>,
    rollback_core: Option<ModalCoreId>,
}

struct ActualTrace {
    binding: shadow::Binding<ActualContext>,
    stock: shadow::StockReturn,
    events: Vec<PreservationEvidence>,
    registers: [u32; 16],
    cpsr: u32,
    frame: SavedFrame,
    context: ActualContext,
}

fn actual_state(registers: [u32; 16], cpsr: u32, spsr: u32, svc_spsr: u32,
    irq_spsr: u32, svc_sp: u32, irq_sp: u32, svc_lr: u32, irq_lr: u32) -> Arm32State {
    Arm32State { registers, cpsr, spsr, svc_spsr, irq_spsr, svc_sp, irq_sp, svc_lr, irq_lr }
}
fn task_value(task: TaskExecution) -> Value {
    let identity: TaskIdentity = task.identity();
    let stack = task.stack().physical();
    json!({
        "kernel":match identity.kernel { KernelId::Primary => "primary", KernelId::Secondary => "secondary" },
        "id":identity.id.get(),
        "node":identity.node.address().0,
        "creation":identity.creation.address().0,
        "stack":{"start":stack.start().0,"end":stack.end().0},
        "execution_core":task.execution_core().0
    })
}

fn phase_name(phase: LifetimePhase) -> &'static str {
    match phase {
        LifetimePhase::Prepared => "prepared",
        LifetimePhase::Publishing => "publishing",
        LifetimePhase::Installed => "installed",
        LifetimePhase::RollingBack => "rolling_back",
        LifetimePhase::RollbackStored => "rollback_stored",
        LifetimePhase::RollbackVerified => "rollback_verified",
        LifetimePhase::Ended => "ended",
    }
}


// Copied from 20261115T131950Z-menu-irq-vector-restored6's first_irq packet,
// e2e-modal-observation.json SHA-256 23bb11e1206b66199a475c1f2c540f1ae19e54734b6b4d4bdee1f7c51ba8f312.
fn actual_menu_trace() -> Result<ActualTrace, String> {
    let owner = Identity::allocate();
    let serial = |value| std::num::NonZeroU64::new(value).expect("actual evidence serial is nonzero");
    // Entry IDs are opaque; preserve their same-owner/resume relation while the report retains original serials.
    let entry_id = InstructionEntryId::after(owner, None).expect("fresh synthetic entry identity");
    let observed_id = InstructionEntryId::after(owner, Some(entry_id)).expect("resumed synthetic entry identity");
    let tcm = [None, Some(InstructionTcm { address: 0x70000000, bytes: 0x2000 })];
    let entry = InstructionEntry {
        id: entry_id, address: 12562472, bytes: 4, progress: 6827, pstate: 0x80000153,
        physical: Some(12562472), tcm,
    };
    let observed = InstructionEntry {
        id: observed_id, address: 12562472, bytes: 4, progress: 1, pstate: 0x80000153,
        physical: Some(12562472), tcm,
    };
    let before = actual_state(
        [0, 2573, 48, 21395172, 0, 9717120, 1, 9717124, 1074099628, 9,
            1074087392, 10800572, 5, 10800568, 12565776, 12562472],
        0x80000153, 0x53, 0x53, 0x53, 10555872, 211, 283632, 6933504);
    let retired_after = actual_state(
        [0, 2573, 48, 21395172, 0, 9717120, 1, 9717124, 1074099628, 9,
            1074087392, 10800572, 5, 10800532, 12565776, 12562476],
        0x80000153, 0x53, 0x53, 0x53, 10555872, 211, 283632, 6933504);
    let frame = SavedFrame { address: 10800532, physical: 10800532,
        words: [0, 9717120, 1, 9717124, 1074099628, 9, 1074087392, 10800572, 12565776] };
    let irq_after = actual_state(
        [0, 2573, 48, 21395172, 0, 9717120, 1, 9717124, 1074099628, 9,
            1074087392, 10800572, 5, 211, 12562480, 24],
        0x800001d2, 0x80000153, 0x53, 0x53, 10800532, 211, 12565776, 6933504);
    let status_before = actual_state(
        [0, 0x80000153, 64, 0, 0, 9717120, 1, 9717124, 1074099628, 9,
            1074087392, 10800572, 4294656000, 10800504, 262928, 263008],
        0x600001d3, 0x53, 0x53, 0x80000153, 10800504, 211, 12565776, 12562480);
    let mut status_after = status_before;
    status_after.spsr = 0x80000053;
    status_after.registers[15] = 263012;
    let returned_after = actual_state(
        [0, 2573, 48, 21395172, 0, 9717120, 1, 9717124, 1074099628, 9,
            1074087392, 10800572, 5, 10800532, 12565776, 12562476],
        0x80000053, 0x80000053, 0x53, 0x80000153, 10800504, 211, 12565776, 12562480);
    let push = ExecutionSource { address: 12562472, physical: 12562472, word: 0xe92d4ff0, tcm };
    let status = ExecutionSource { address: 263008, physical: 263008, word: 0xe169f001, tcm };
    let returning = ExecutionSource { address: 263012, physical: 263012, word: 0xe8fdd00f, tcm };
    let exception = EvidenceId { owner, serial: serial(65625) };
    let events = vec![
        PreservationEvidence::Retired(InstructionRetirement {
            execution: EvidenceId { owner, serial: serial(65624) },
            observation: Some(observed), resumed_from: Some(entry), source: push,
            before, after: retired_after, frame,
        }),
        PreservationEvidence::Entered(ExceptionEntry {
            execution: exception, before: retired_after, after: irq_after, vector: 24,
        }),
        PreservationEvidence::SavedStatus(SavedStatusWrite {
            exception, source: status, before: status_before, after: status_after,
        }),
        PreservationEvidence::Returned(ExceptionReturn {
            exception, source: returning, before: status_after, after: returned_after,
            written_status: 0x80000053, mask: 0xff1fffff,
        }),
    ];
    let task = copied_task(KernelId::Primary, ACTUAL_CREATION_WORDS)?;
    let identity = task.identity();
    if identity.id.get() != 1 || identity.node.address() != Guest(1074134796)
        || identity.creation.address() != Guest(1074118316) {
        return Err("canonical task sample differs from retained Menu identity".into());
    }
    let context = ActualContext {
        task,
        phase: LifetimePhase::RollbackStored,
        forward: PublicationCount(226),
        reverse: PublicationCount(226),
        install_core: Some(ModalCoreId(0)),
        rollback_core: Some(ModalCoreId(0)),
    };
    Ok(ActualTrace {
        binding: shadow::Binding { entry, registers: before.registers, cpsr: before.cpsr, context },
        stock: shadow::StockReturn { vector: 24, status, returning },
        events, registers: returned_after.registers, cpsr: returned_after.cpsr, frame, context,
    })
}

fn actual_refusal(controls: &mut serde_json::Map<String, Value>, name: &str,
    trace: &ActualTrace, events: &[PreservationEvidence], registers: [u32; 16],
    cpsr: u32, context: ActualContext, frame: SavedFrame) -> Result<(), String> {
    let failure = judge_bound(trace.binding, trace.stock, events, registers, cpsr, context, frame)
        .err().ok_or_else(|| format!("{name} actual evidence mutation was admitted"))?;
    controls.insert(name.into(), json!({"refused":true,"failure":format!("{failure:?}")}));
    Ok(())
}

fn completed_actual(trace: &ActualTrace) -> Result<shadow::Shadow<ActualContext>, String> {
    let mut verifier = shadow::Shadow::new(trace.binding, trace.stock)
        .map_err(|failure| format!("actual binding refused: {failure:?}"))?;
    for event in &trace.events {
        verifier.observe(*event).map_err(|failure| format!("actual event refused: {failure:?}"))?;
    }
    verifier.handoff(trace.registers, trace.cpsr, trace.frame, trace.context)
        .map_err(|failure| format!("actual handoff refused: {failure:?}"))?;
    verifier.finish().map_err(|failure| format!("actual finish refused: {failure:?}"))?;
    Ok(verifier)
}

fn actual_menu_controls() -> Result<Value, String> {
    let trace = actual_menu_trace()?;
    judge_bound(trace.binding, trace.stock, &trace.events, trace.registers, trace.cpsr,
        trace.context, trace.frame).map_err(|failure| format!("actual positive refused: {failure:?}"))?;
    let mut controls = serde_json::Map::new();

    actual_refusal(&mut controls, "missing-saved-status", &trace,
        &[trace.events[0], trace.events[1], trace.events[3]], trace.registers,
        trace.cpsr, trace.context, trace.frame)?;
    actual_refusal(&mut controls, "duplicate-saved-status", &trace,
        &[trace.events[0], trace.events[1], trace.events[2], trace.events[2], trace.events[3]],
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    actual_refusal(&mut controls, "return-before-saved-status", &trace,
        &[trace.events[0], trace.events[1], trace.events[3], trace.events[2]],
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    actual_refusal(&mut controls, "missing-return", &trace, &trace.events[..3],
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    actual_refusal(&mut controls, "duplicate-return", &trace,
        &[trace.events[0], trace.events[1], trace.events[2], trace.events[3], trace.events[3]],
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    actual_refusal(&mut controls, "retirement-after-entry", &trace,
        &[trace.events[1], trace.events[0], trace.events[2], trace.events[3]],
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    actual_refusal(&mut controls, "destination-before-retirement", &trace, &[],
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    actual_refusal(&mut controls, "destination-during-open-irq", &trace, &trace.events[..2],
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    actual_refusal(&mut controls, "destination-before-return", &trace, &trace.events[..3],
        trace.registers, trace.cpsr, trace.context, trace.frame)?;

    let mut changed = trace.events.clone();
    let PreservationEvidence::Returned(returned) = &mut changed[3] else { unreachable!() };
    returned.after.registers[6] ^= 1;
    actual_refusal(&mut controls, "returned-gpr", &trace, &changed,
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    let mut registers = trace.registers;
    registers[6] ^= 1;
    actual_refusal(&mut controls, "late-gpr", &trace, &trace.events,
        registers, trace.cpsr, trace.context, trace.frame)?;
    let mut changed = trace.events.clone();
    let PreservationEvidence::Retired(retired) = &mut changed[0] else { unreachable!() };
    retired.frame.address ^= 4;
    actual_refusal(&mut controls, "retired-frame-address", &trace, &changed,
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    let mut frame = trace.frame;
    frame.physical ^= 4;
    actual_refusal(&mut controls, "late-frame-physical", &trace, &trace.events,
        trace.registers, trace.cpsr, trace.context, frame)?;
    let mut changed = trace.events.clone();
    let PreservationEvidence::Retired(retired) = &mut changed[0] else { unreachable!() };
    retired.resumed_from = None;
    actual_refusal(&mut controls, "resume-lineage", &trace, &changed,
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    let mut changed = trace.events.clone();
    let PreservationEvidence::Retired(retired) = &mut changed[0] else { unreachable!() };
    retired.source.physical ^= 4;
    actual_refusal(&mut controls, "source-physical", &trace, &changed,
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    let mut changed = trace.events.clone();
    let PreservationEvidence::Retired(retired) = &mut changed[0] else { unreachable!() };
    retired.source.tcm = [None, None];
    actual_refusal(&mut controls, "source-tcm", &trace, &changed,
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    let mut changed = trace.events.clone();
    let PreservationEvidence::Entered(entered) = &mut changed[1] else { unreachable!() };
    entered.vector = 28;
    actual_refusal(&mut controls, "exception-vector", &trace, &changed,
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    let mut changed = trace.events.clone();
    let PreservationEvidence::SavedStatus(status) = &mut changed[2] else { unreachable!() };
    status.source.physical ^= 4;
    actual_refusal(&mut controls, "status-source-physical", &trace, &changed,
        trace.registers, trace.cpsr, trace.context, trace.frame)?;
    let mut changed = trace.events.clone();
    let PreservationEvidence::Returned(returned) = &mut changed[3] else { unreachable!() };
    returned.source.physical ^= 4;
    actual_refusal(&mut controls, "return-source-physical", &trace, &changed,
        trace.registers, trace.cpsr, trace.context, trace.frame)?;

    let mut alternate_words = ACTUAL_CREATION_WORDS;
    alternate_words[4] = 3584;
    alternate_words[5] = 10797056;
    let task = ActualContext {
        task: copied_task(KernelId::Primary, alternate_words)?,
        ..trace.context
    };
    actual_refusal(&mut controls, "menu-task-substitution", &trace, &trace.events,
        trace.registers, trace.cpsr, task, trace.frame)?;
    let wrong_kernel = ActualContext {
        task: copied_task(KernelId::Secondary, ACTUAL_CREATION_WORDS)?,
        ..trace.context
    };
    actual_refusal(&mut controls, "menu-wrong-kernel-identity", &trace, &trace.events,
        trace.registers, trace.cpsr, wrong_kernel, trace.frame)?;
    let publication = ActualContext { forward: PublicationCount(227), ..trace.context };
    actual_refusal(&mut controls, "menu-publication-substitution", &trace, &trace.events,
        trace.registers, trace.cpsr, publication, trace.frame)?;

    let mut completed = completed_actual(&trace)?;
    let failure = completed.observe(trace.events[0]).err()
        .ok_or("late retirement after completed actual evidence was admitted")?;
    controls.insert("retirement-after-completion".into(),
        json!({"refused":true,"failure":format!("{failure:?}")}));
    let mut completed = completed_actual(&trace)?;
    let failure = completed.observe(trace.events[3]).err()
        .ok_or("late return after completed actual evidence was admitted")?;
    controls.insert("return-after-completion".into(),
        json!({"refused":true,"failure":format!("{failure:?}")}));
    let mut completed = completed_actual(&trace)?;
    let failure = completed.handoff(trace.registers, trace.cpsr, trace.frame, trace.context).err()
        .ok_or("duplicate destination after completed actual evidence was admitted")?;
    controls.insert("destination-after-completion".into(),
        json!({"refused":true,"failure":format!("{failure:?}")}));

    let passed = controls.len() == 25 && controls.values().all(|control| control["refused"] == true);
    Ok(json!({
        "case":"actual-menu-causal-controls",
        "passed":passed,
        "positive":true,
        "controls":controls,
        "fixture_provenance":{
            "kind":"copied-adversarial-host-fixture",
            "source":"retained actual Menu first_irq artifact",
            "source_sha256":"23bb11e1206b66199a475c1f2c540f1ae19e54734b6b4d4bdee1f7c51ba8f312",
            "construction":"fujimod_xm1::modal_task::sample over explicit ValidatedTaskMemory",
            "unknown_memory":"MemoryError::Unmapped",
            "original_stopped_entry_serial":799689u64,
            "retirement_serial":65624u64,
            "exception_serial":65625u64,
            "task":task_value(trace.context.task),
            "creation_words":ACTUAL_CREATION_WORDS,
            "publication":{
                "phase":phase_name(trace.context.phase),
                "forward":trace.context.forward.0,
                "reverse":trace.context.reverse.0,
                "install_core":trace.context.install_core.map(|core|core.0),
                "rollback_core":trace.context.rollback_core.map(|core|core.0)
            }
        },
        "scope":{
            "inner_verifier":"unchanged fujimod_emulator_cpu::preservation::shadow::Shadow",
            "diagnostic_wrapper":"not exercised; packets without an active shadow are outside this proof",
            "production_certificate_path":false,
            "production_guard_changed":false,
            "camera_operation":false
        },
        "original_events":trace.events.iter().map(|event|format!("{event:?}")).collect::<Vec<_>>()
    }))
}

fn run_controls() -> Result<Value, String> {
    let mut roles = Vec::new();
    let mut controls = serde_json::Map::new();
    let fault = faulted_native()?;
    eprintln!("NATIVE_CONTROL actual-faulted-retirement {fault}");
    controls.insert("actual-faulted-retirement".into(), fault);
    for worker in [false, true] { for context in [false, true] {
        let enabled = execute(worker, context, true, "none")?;
        judge(&enabled, &enabled.events, enabled.binding.context, enabled.frame)
            .map_err(|e|format!("positive: {e:?}; raw native events={:?}", enabled.events))?;
        let disabled = execute(worker, context, false, "none")?;
        if enabled.pending != disabled.pending || enabled.finished != disabled.finished {
            return Err(format!("enabled/disabled full Board state differs; enabled_pending={} disabled_pending={} enabled_final={} disabled_final={}",
                enabled.pending, disabled.pending, enabled.finished, disabled.finished));
        }
        roles.push(json!({"worker":worker,"context_adapter":context,"positive":true,
            "full_pending_state_equal":true,"full_final_state_equal":true,
            "pending":enabled.pending,"finished":enabled.finished,
            "disabled_pending":disabled.pending,"disabled_finished":disabled.finished,
            "events":enabled.events.iter().map(|event|format!("{event:?}")).collect::<Vec<_>>()}));
        eprintln!("NATIVE_ROLE {}", roles.last().unwrap());
        if !worker && !context {
            for injection in ["a-before-retirement", "a-before-irq", "a-after-return",
                "saved-status-before-write", "wrong-bank-before-write", "flags-after-return"] {
                let negative = execute(false, false, true, injection)?;
                controls.insert(injection.into(), refused(&negative, &negative.events, negative.binding.context, negative.frame)?);
                eprintln!("NATIVE_CONTROL {injection} {}", controls[injection]);
            }
            let mut cases = Vec::new();
            let mut changed = enabled.events.clone();
            let PreservationEvidence::SavedStatus(write) = &mut changed[2] else {return Err("missing native saved status".into());};
            write.before.registers[1] ^= 0x100;
            cases.push(("corrupt-saved-status", changed));
            let mut changed = enabled.events.clone();
            let PreservationEvidence::SavedStatus(write) = &mut changed[2] else {unreachable!()};
            write.before.cpsr = (write.before.cpsr & !0x1f) | 0x12;
            cases.push(("wrong-bank", changed));
            let mut changed = enabled.events.clone();
            let PreservationEvidence::Returned(returned) = &mut changed[3] else {return Err("missing native return".into());};
            returned.after.cpsr ^= 0x80000000;
            cases.push(("unrelated-flags", changed));
            let mut changed = enabled.events.clone();
            let PreservationEvidence::Entered(entry) = &mut changed[1] else {unreachable!()};
            entry.execution.owner = Identity::allocate();
            cases.push(("wrong-core", changed));
            let mut changed = enabled.events.clone();
            let PreservationEvidence::Returned(returned) = &mut changed[3] else {unreachable!()};
            returned.exception.serial = std::num::NonZeroU64::new(999).unwrap();
            cases.push(("forged-exception", changed));
            cases.push(("missing-exception", vec![enabled.events[0], enabled.events[2], enabled.events[3]]));
            cases.push(("duplicate-exception", vec![enabled.events[0], enabled.events[1], enabled.events[1], enabled.events[2], enabled.events[3]]));
            cases.push(("unbalanced-exception", enabled.events[..3].to_vec()));
            cases.push(("missing-retirement", enabled.events[1..].to_vec()));
            cases.push(("late-retirement", vec![enabled.events[1], enabled.events[0], enabled.events[2], enabled.events[3]]));
            cases.push(("duplicate-retirement", vec![enabled.events[0], enabled.events[0]]));
            cases.push(("faulted-retirement", vec![PreservationEvidence::Refused {address:0x1000,reason:EvidenceFailure::Faulted}]));
            let mut changed = enabled.events.clone();
            let PreservationEvidence::Retired(retired) = &mut changed[0] else {unreachable!()};
            retired.frame.words[8] ^= 4;
            cases.push(("incomplete-frame", changed));
            for (name, events) in cases { controls.insert(name.into(), refused(&enabled, &events, enabled.binding.context, enabled.frame)?); }
            controls.insert("task-context".into(), refused(&enabled, &enabled.events, (2,1), enabled.frame)?);
            controls.insert("publication-context".into(), refused(&enabled, &enabled.events, (1,2), enabled.frame)?);
        }
    }}
    Ok(json!({"scope":"synthetic-native-control","roles":roles,"controls":controls,
        "production_guard_changed":false,"camera_operation":false,"value_qualification":false}))
}
unsafe fn retry_store(access: Access, address: u64, size: i32, value: i64, data: *mut std::ffi::c_void) {
    eprintln!("retry committed store {access:?} {address:#x} {size} {value:#x}");
    assert!(matches!(access, Access::Write) && matches!(address, 0x1000 | 0x8000)
        && size == 4 && value as u32 == 0xe320f000);
    let count = unsafe { &*data.cast::<Cell<u64>>() };
    count.set(count.get() + 1);
}

fn restart_lifecycle(worker: bool, context_adapter: bool) -> Result<Value, String> {
    let stores = Box::new(Cell::new(0u64));
    let mut board = board(worker, true)?;
    words(&board, 0x18, &[0xe58c2000, 0xea0007f7])?;
    set(board.cpu(0)?, arm::CORE[12], 0x8000)?;
    set(board.cpu(0)?, arm::CORE[2], 0xe320f000)?;
    let events = Rc::new(RefCell::new(Vec::with_capacity(32)));
    let observed = events.clone();
    let callback = Rc::new(move |_, event| {
        eprintln!("retry native {event:?}");
        let mut events = observed.borrow_mut();
        assert!(events.len() < events.capacity());
        events.push(event);
    });
    let range = HookRange::Inclusive { begin: 0x1000, end: 0x1000 };
    let mut watch = if context_adapter {
        board.observation_context().preservation_evidence_begin(0, range, callback)?
    } else { board.preservation_evidence_begin(0, range, callback)? };
    let data = (&*stores as *const Cell<u64>).cast_mut().cast();
    let store_hooks = [0x1000, 0x8000].map(|begin| unsafe {
        board.cpu(0)?.install_hook(Hook::CompletedStore { callback: retry_store, data,
            range: HookRange::Inclusive { begin, end: begin + 3 } })
    }).into_iter().collect::<Result<Vec<_>, String>>()?;
    let mut points = board.instruction_points_begin(0, vec![0x1000, 0x1004])?;
    let run = |begin, instructions| Run { begin, until: None, watchdog_us: 0, instructions: Some(instructions) };
    board.cpu(0)?.run(run(0x1000, 1)).map_err(|e|e.to_string())?;
    if points.event() != Some(0x1000) || !events.borrow().is_empty() {
        return Err("retry control did not reach unexecuted PUSH".into());
    }
    points.resume(0x1000, 0x1000);
    board.cpu(0)?.run(run(0x1000, 1)).map_err(|e|e.to_string())?;
    board.cpu(0)?.set_interrupt(Interrupt::Irq, true)?;
    board.cpu(0)?.run(run(0x1004, 4)).map_err(|e|e.to_string())?;
    board.cpu(0)?.set_interrupt(Interrupt::Irq, false)?;
    let begin = u64::from(get(board.cpu(0)?, arm::PC)?);
    board.cpu(0)?.run(Run { until: Some(0x1004), ..run(begin, 90) }).map_err(|e|e.to_string())?;
    let evidence = events.borrow().clone();
    let mut written = [0; 4];
    board.cpu(0)?.read_memory(0x1000, &mut written)?;
    let passed = u32::from_le_bytes(written) == 0xe320f000 && stores.get() == 1
        && evidence.iter().any(|e|matches!(e, PreservationEvidence::Entered(_)))
        && evidence.iter().any(|e|matches!(e, PreservationEvidence::Refused { reason: EvidenceFailure::Restarted, .. }))
        && evidence.iter().any(|e|matches!(e, PreservationEvidence::Refused { reason: EvidenceFailure::UnpairedException, .. }))
        && !evidence.iter().any(|e|matches!(e, PreservationEvidence::SavedStatus(_) | PreservationEvidence::Returned(_)));
    let fingerprint = board.cpu(0)?.fingerprint()?;
    let backend_counters = board.cpu(0)?.exit_counters()?;
    watch.remove()?;
    board.instruction_points_end(0, &mut points)?;
    board.cpu(0)?.remove_hooks(&store_hooks)?;
    Ok(json!({"case":"restart-open-irq","worker":worker,"context_adapter":context_adapter,"passed":passed,
        "standalone_cpu_requests":true,"instruction_ceiling":100,"alias_written":written,"fingerprint":fingerprint,
        "completed_stores":stores.get(),"backend_counters":backend_counters,
        "events":evidence.iter().map(|e|format!("{e:?}")).collect::<Vec<_>>()}))
}

fn lifecycle(case: &str, worker: bool, context_adapter: bool) -> Result<Value, String> {
    if case == "partial-store" { return partial_store(worker, context_adapter); }
    if case == "restart-open-irq" { return restart_lifecycle(worker, context_adapter); }
    let mut board = board(worker, false)?;
    words(&board, 0x3000, &[0xe320f000])?;
    if case == "fault-open-irq" {
        words(&board, 0x2000, &[0xe59c0000])?;
        set(board.cpu(0)?, arm::CORE[12], 0x90000000)?;
    }
    let snapshot_fingerprint = board.cpu(0)?.fingerprint()?;
    let snapshot = board.cpu(0)?.snapshot(None)?;
    let events = Rc::new(RefCell::new(Vec::with_capacity(64)));
    let observed = events.clone();
    let callback = Rc::new(move |_: fujimod_emulator::board::observation::CoreIndex, event| {
        eprintln!("lifecycle native {event:?}");
        let mut events = observed.borrow_mut();
        assert!(events.len() < events.capacity(), "lifecycle evidence capacity exhausted");
        events.push(event);
    });
    let range = HookRange::Inclusive { begin: 0x1000, end: 0x1000 };
    let mut watch = if context_adapter {
        board.observation_context().preservation_evidence_begin(0, range, callback)?
    } else { board.preservation_evidence_begin(0, range, callback)? };
    let mut points = board.instruction_points_begin(0, vec![0x1000, 0x1004])?;
    board.run_until(2, 1)?;
    if points.event() != Some(0x1000) || !events.borrow().is_empty() {
        return Err("lifecycle did not stop before an unexecuted PUSH".into());
    }
    let entry = points.event_context().ok_or("missing lifecycle entry")?.entry;
    let lineage_binding = shadow::Binding { entry, registers: registers(&board)?,
        cpsr: get(board.cpu(0)?, arm::CPSR)?, context: (1, 1) };
    points.resume(0x1000, 0x1000);
    let run = |begin, instructions| Run { begin, until: None, watchdog_us: 0, instructions: Some(instructions) };
    let mut run_error = None;
    let mut removed = false;
    let passed;
    let mut lineage_accepted = None;
    let mut source_bytes = Vec::new();
    let read_source = || -> Result<[u8; 4], String> {
        let mut bytes = [0; 4];
        board.cpu(0)?.read_memory(0x1000, &mut bytes)?;
        Ok(bytes)
    };
    if matches!(case, "lineage-intervening-run" | "lineage-mapping-change" | "lineage-source-write" | "lineage-physical-write" | "lineage-immediate-resume") {
        if case == "lineage-intervening-run" {
            board.cpu(0)?.run(run(0x3000, 1)).map_err(|e| e.to_string())?;
        } else if case == "lineage-mapping-change" {
            board.cpu(0)?.protect(0x1000, 0x1000, 3)?;
            board.cpu(0)?.protect(0x1000, 0x1000, 7)?;
        } else if case == "lineage-source-write" {
            board.cpu(0)?.write_memory(0x1000, &0xe320f000u32.to_le_bytes())?;
            board.cpu(0)?.write_memory(0x1000, &0xe92d4ff0u32.to_le_bytes())?;
        } else if case == "lineage-physical-write" {
            source_bytes.push(read_source()?);
            words(&board, 0x1000, &[0xe320f000])?;
            source_bytes.push(read_source()?);
            words(&board, 0x1000, &[0xe92d4ff0])?;
            source_bytes.push(read_source()?);
        }
        board.cpu(0)?.run(run(0x1000, 1)).map_err(|e| e.to_string())?;
        let retired = events.borrow().iter().find_map(|event| match event {
            PreservationEvidence::Retired(retired) => Some(*retired), _ => None,
        }).ok_or("lifecycle PUSH did not retire")?;
        let carries_old = retired.resumed_from.is_some_and(|old| old.id == entry.id);
        let source = |address, word| ExecutionSource { address, physical: u64::from(address), word, tcm: entry.tcm };
        let mut verifier = shadow::Shadow::new(lineage_binding, shadow::StockReturn { vector: 0x18,
            status: source(0x2044, 0xe169f001), returning: source(0x2048, 0xe8fdd00f) })
            .map_err(|e|format!("lineage binding: {e:?}"))?;
        let mut bytes = [0; 36];
        board.cpu(0)?.read_memory(0x6fdc, &mut bytes)?;
        let frame = SavedFrame { address: 0x6fdc, physical: 0x6fdc,
            words: std::array::from_fn(|index|u32::from_le_bytes(bytes[index*4..index*4+4].try_into().unwrap())) };
        let late = registers(&board)?;
        let cpsr = get(board.cpu(0)?, arm::CPSR)?;
        let accepted = events.borrow().iter().try_for_each(|event|verifier.observe(*event))
            .and_then(|_|verifier.handoff(late, cpsr, frame, (1,1)))
            .and_then(|_|verifier.finish()).is_ok();
        lineage_accepted = Some(accepted);
        passed = carries_old == (case == "lineage-immediate-resume")
            && accepted == (case == "lineage-immediate-resume");
    } else {
        board.run_until(2, 1)?;
        board.cpu(0)?.set_interrupt(Interrupt::Irq, true)?;
        board.run_until(3, 1)?;
        board.cpu(0)?.set_interrupt(Interrupt::Irq, false)?;
        if !events.borrow().iter().any(|e| matches!(e, PreservationEvidence::Entered(_))) {
            return Err("lifecycle did not enter an actual IRQ".into());
        }
        eprintln!("lifecycle operation {case}: begin");
        match case {
            "remove-open-irq" => {
                watch.remove()?; removed = true;
                let count = events.borrow().len();
                board.run_until(100, 1)?;
                if events.borrow().len() != count {
                    return Err("removed watch still delivered native IRQ evidence".into());
                }
            }
            "restore-open-irq" => {
                board.cpu(0)?.restore(&*snapshot)?;
                if board.cpu(0)?.fingerprint()? != snapshot_fingerprint {
                    return Err("snapshot restore replied without restoring its CPU state".into());
                }
            }
            "fault-open-irq" => {
                run_error = board.run_until(100, 1).err();
                let refused = events.borrow().iter().any(|event| matches!(event,
                    PreservationEvidence::Refused { reason: EvidenceFailure::Faulted, .. }));
                let recovery_error = board.cpu(0)?.run(run(0x2004, 100)).err();
                let unpaired = events.borrow().iter().any(|event| matches!(event,
                    PreservationEvidence::Refused { reason: EvidenceFailure::UnpairedException, .. }));
                passed = run_error.is_some() && recovery_error.is_none() && refused && unpaired
                    && !events.borrow().iter().any(|event|
                        matches!(event, PreservationEvidence::SavedStatus(_) | PreservationEvidence::Returned(_)));
                let raw = events.borrow().iter().map(|event| format!("{event:?}")).collect::<Vec<_>>();
                let fingerprint = board.fingerprint()?;
                watch.remove()?;
                board.instruction_points_end(0, &mut points)?;
                return Ok(json!({"case":case,"worker":worker,"context_adapter":context_adapter,
                    "passed":passed,"run_error":run_error,"recovery_error":recovery_error.map(|e|e.to_string()),"events":raw,"fingerprint":fingerprint}));
            }
            "bounded-irq-slices" => {
                for deadline in 4..100 { board.run_until(deadline, 1)?; }
            }
            _ => return Err("unknown lifecycle control".into()),
        }
        eprintln!("lifecycle operation {case}: complete");
        passed = if case == "bounded-irq-slices" {
            let evidence = events.borrow();
            evidence.iter().filter(|e| matches!(e, PreservationEvidence::Entered(_))).count() == 1
                && evidence.iter().filter(|e| matches!(e, PreservationEvidence::Returned(_))).count() == 1
                && !evidence.iter().any(|e| matches!(e, PreservationEvidence::Refused { .. }))
        } else {
            events.borrow().iter().any(|e| matches!(e,
                PreservationEvidence::Refused { reason: EvidenceFailure::SourceChanged, .. }))
        };
    }
    let raw = events.borrow().iter().map(|event| format!("{event:?}")).collect::<Vec<_>>();
    let fingerprint = board.fingerprint()?;
    if !removed { watch.remove()?; }
    board.instruction_points_end(0, &mut points)?;
    Ok(json!({"case":case,"worker":worker,"context_adapter":context_adapter,"passed":passed,
        "run_error":run_error,"events":raw,"fingerprint":fingerprint,
        "entry":format!("{entry:?}"),"source_bytes":source_bytes,"lineage_accepted":lineage_accepted}))
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [] => run_controls(),
        [mode] if mode == "--bank-controls" => bank_controls(),
        [mode] if mode == "--actual-menu-controls" => actual_menu_controls(),
        [mode, case, execution, adapter] if mode == "--lifecycle"
            && matches!(execution.as_str(), "direct" | "worker")
            && matches!(adapter.as_str(), "board" | "context") =>
                lifecycle(case, execution == "worker", adapter == "context"),
        _ => Err("expected --actual-menu-controls, --bank-controls or --lifecycle CASE direct|worker board|context".into()),
    };
    match result {
        Ok(value) => {
            println!("{}", serde_json::to_string(&value).unwrap());
            if value.get("passed") == Some(&Value::Bool(false)) { std::process::exit(1); }
        }
        Err(error) => { eprintln!("{error}"); std::process::exit(1); }
    }
}
