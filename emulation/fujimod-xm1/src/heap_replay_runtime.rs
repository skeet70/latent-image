use crate::heap_accesses::Digest;
use crate::heap_replay_contract::*;
use crate::heap_replay_state::{AllocationOutcome, Bytes, Layout, Request, Tape};
use crate::heap_state::{Address, AllocationId};
use fujimod_emulator::board::{Board, factory::MemoryMap, harness, observation::{MemoryWatch, ViewPolicy, WatchKinds, WatchRange}};
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{Register, arm};
use fujimod_xm1::{bridge::sha256, r4f::Guard, run::{self, RunDocument, ValidatedUpdate}};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};

#[path = "memory_view.rs"]
mod memory_view;
use memory_view::{MemoryAddress, MemoryCore, TcmRegister, static_span, tcm_window};

const CALLOC: Span = Span { start: Address(0xbf1c1c), end: Address(0xbf1cd4) };

fn execution(detail: String) -> Failure { Failure::Execution { detail } }
fn require(condition: bool, failure: Failure) -> Result<(), Failure> {
    if condition { Ok(()) } else { Err(failure) }
}
fn save(path: &Path, value: &impl Serialize) -> Result<(), Failure> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
fn register(board: &Board, core: usize, register: arm::Register) -> Result<u32, Failure> {
    u32::try_from(board.read_register(core, Register::Arm32(register)).map_err(execution)?).map_err(|_| Failure::Context)
}
fn write_register(board: &Board, register: arm::Register, word: u32) -> Result<(), Failure> {
    board.write_register(0, Register::Arm32(register), u64::from(word)).map_err(execution)
}
fn physical(maps: &[MemoryMap], address: Address, length: usize) -> Result<u64, Failure> {
    let end = u64::from(address.0).checked_add(length as u64).ok_or(Failure::CodeExtent)?;
    static_span(maps, MemoryCore(0), MemoryAddress(u64::from(address.0))..MemoryAddress(end))
        .map(|span| span.physical.0).map_err(execution)
}
fn checked_read(board: &Board, maps: &[MemoryMap], address: Address, bytes: &mut [u8], scratch: &mut [u8]) -> Result<(), Failure> {
    require(bytes.len() == scratch.len(), Failure::Data)?;
    if bytes.is_empty() { return Ok(()); }
    let start = u64::from(address.0);
    let end = start.checked_add(bytes.len() as u64).ok_or(Failure::CodeExtent)?;
    for core in 0..2 {
        for bank in 0..2 {
            let value = board.cpu(core).map_err(execution)?.coprocessor(arm::Coprocessor {
                cp: 15, opc1: 0, crn: 9, crm: 1, opc2: bank, secure: 0, wide: false,
            }, None).map_err(execution)?;
            let value = u32::try_from(value).map_err(|_| Failure::Context)?;
            if let Some((low, high)) = tcm_window(TcmRegister(value)).map_err(execution)? {
                require(end <= low || start >= high, Failure::Context)?;
            }
        }
    }
    board.read_memory(0, start, bytes).map_err(execution)?;
    board.read_physical(physical(maps, address, bytes.len())?, scratch).map_err(execution)?;
    require(bytes == scratch, Failure::Data)
}
fn read_into(board: &Board, maps: &[MemoryMap], address: Address, bytes: &mut [u8], scratch: &mut Vec<u8>) -> Result<(), Failure> {
    scratch.resize(bytes.len(), 0);
    checked_read(board, maps, address, bytes, scratch)
}
fn word(board: &Board, maps: &[MemoryMap], address: u32) -> Result<Word, Failure> {
    let mut bytes = [0; 4];
    let mut scratch = [0; 4];
    checked_read(board, maps, Address(address), &mut bytes, &mut scratch)?;
    Ok(Word(u32::from_le_bytes(bytes)))
}
fn frame(board: &Board, maps: &[MemoryMap]) -> Result<CpuFrame, Failure> {
    let mut registers = [0; 16];
    for (index, value) in registers.iter_mut().enumerate() { *value = register(board, 0, arm::Register::R(index as u8))?; }
    let counts: [u64; 2] = board.instruction_counts().try_into().map_err(|_| Failure::Clock)?;
    Ok(CpuFrame { registers, d8_d15: vfp(board)?.map(RegisterBits), cpsr: Word(register(board, 0, arm::Register::Cpsr)?), counts: counts.map(Instructions),
        root: Address(word(board, maps, 0x96bd20)?.0), task_record: Address(word(board, maps, 0x40054760)?.0),
        kernel_context: word(board, maps, 0x400545a4)?, identity: Word(board.cpu(0).map_err(execution)?.rprofile_identity(None).map_err(execution)?),
        owner: word(board, maps, 0x40a6c7d8)?, depth: word(board, maps, 0x40a6c7dc)?, heap_break: Address(word(board, maps, 0x829a08)?.0) })
}
fn preserved(before: &CpuFrame, after: &CpuFrame) -> Result<(), Failure> {
    require(before.registers[4..12] == after.registers[4..12] && before.registers[13] == after.registers[13]
        && before.d8_d15 == after.d8_d15, Failure::Aapcs)?;
    require(before.cpsr.0 & 0xff == 0x53 && before.cpsr.0 & 0x3ff == after.cpsr.0 & 0x3ff && before.root == after.root
        && before.task_record == after.task_record && before.kernel_context == after.kernel_context
        && before.identity == after.identity && before.owner == after.owner && before.depth == after.depth
        && before.counts[1] == after.counts[1], Failure::Context)
}
fn vfp(board: &Board) -> Result<[u64; 8], Failure> {
    let mut words = [0; 8];
    for (index, value) in words.iter_mut().enumerate() {
        *value = board.read_register(0, Register::Arm32(arm::Register::D(index as u8 + 8))).map_err(execution)?;
    }
    Ok(words)
}
#[derive(Clone, Copy)]
struct Live { pointer: Address, layout: Layout, footprint: Bytes }
struct ProcessorQuantum(std::num::NonZeroUsize);
struct Runtime<'a> {
    inputs: &'a Inputs,
    output: &'a Path,
    session: Session,
    maps: Vec<MemoryMap>,
    guard: Guard,
    watch: Option<Box<MemoryWatch>>,
    ranges: Vec<(u64, u64)>,
    live: BTreeMap<AllocationId, Live>,
    scratch: Vec<u8>,
    physical_scratch: Vec<u8>,
    peer_identity: u32,
    quantum: ProcessorQuantum,
}
impl Runtime<'_> {
    fn call(&mut self, case: &mut Case, role: CallRole, function: Address, arguments: [u32; 4], candidate: bool) -> Result<CallId, Failure> {
        let board = &self.session.board;
        for (index, value) in arguments.into_iter().enumerate() { write_register(board, arm::Register::R(index as u8), value)?; }
        write_register(board, arm::Register::R(13), STACK_END)?;
        write_register(board, arm::Register::R(14), RETURN_ADDRESS)?;
        write_register(board, arm::Register::R(15), function.0)?;
        let before = frame(board, &self.maps)?;
        let before_fpscr = register(board, 0, arm::Register::Fpscr)?;
        let identity = CallId(case.calls.len());
        let options = harness::Options { core: 0, inject_interrupts: false, deliver_faults: false,
            memory: harness::Memory { aliases: self.maps.iter().filter(|map| map.core == 0).map(|map| harness::Alias {
                address: map.address, physical: map.physical, size: map.size as u64 }).collect(), observed: Vec::new() },
            return_address: u64::from(RETURN_ADDRESS), breakpoints: Vec::new(), stop_addresses: Vec::new(), unarmed: Vec::new(),
            ranges: Some(self.ranges.clone()), vectors: Vec::new(), vector_opcode: 0, stack: (u64::from(STACK_START), u64::from(STACK_END)),
            allowed_stores: None, record_stores: false, include_stack: false, store_limit: 1, coverage: false,
            count_ranges: self.ranges.clone(), trace: 0 };
        board.harness_select_core(0).map_err(execution)?;
        let mut harness = board.harness_begin(options).map_err(execution)?;
        board.harness_interrupt_delivery(0, false);
        board.harness_interrupt_delivery(1, false);
        let end = before.counts[0].0.checked_add(before.counts[1].0).and_then(|total| total.checked_add(CALL_BUDGET)).ok_or(Failure::Clock)?;
        let run_result = self.session.board.run_window(end, self.quantum.0.get(), None, None).map_err(execution);
        let status_word = candidate.then(|| word(&self.session.board, &self.maps, self.inputs.symbol(Symbol::Status)?.0)).transpose()?;
        let raw = serde_json::to_vec(&harness.state())?;
        let end_result = self.session.board.harness_end(&mut harness).map_err(execution);
        let scan_result = self.guard.scan(&self.session.board).map_err(execution);
        let after = frame(&self.session.board, &self.maps)?;
        let after_fpscr = register(&self.session.board, 0, arm::Register::Fpscr)?;
        let preserve_failure = |before: &CpuFrame, after: &CpuFrame| -> Result<(), Failure> {
            std::fs::write(self.output.join(format!("call-{}-harness.json", identity.0)), &raw)?;
            save(&self.output.join(format!("call-{}-frames.json", identity.0)),
                &(before, after, before_fpscr, after_fpscr, status_word))
        };
        let decoded = (|| -> Result<_, Failure> { Ok((serde_json::from_slice::<Harness>(&raw)?, status_word.map(TargetStatus::parse).transpose()?)) })();
        let (observed, status) = match decoded {
            Ok(decoded) => decoded,
            Err(error) => { preserve_failure(&before, &after)?; return Err(error); }
        };
        let count_ranges = self.ranges.iter().map(|&(start, end)| Span { start: Address(start as u32), end: Address(end as u32) }).collect();
        let call = Call { identity, role, function, arguments: arguments.map(Word), before, after, status, harness: observed, count_ranges };
        let validation = (|| -> Result<(), Failure> {
            run_result?;
            end_result?;
            scan_result?;
            preserved(&call.before, &call.after)?;
            require(before_fpscr & 0x07c09f00 == after_fpscr & 0x07c09f00, Failure::Aapcs)?;
            require(self.session.board.cpu(1).map_err(execution)?.rprofile_identity(None).map_err(execution)? == self.peer_identity, Failure::Context)?;
            require(matches!(call.harness.event, Some(HarnessEvent::Return { pc }) if pc.0 == RETURN_ADDRESS)
                && call.after.registers[15] == RETURN_ADDRESS, Failure::Return)?;
            let elapsed = call.elapsed()?.0;
            require(elapsed > 0 && elapsed <= CALL_BUDGET, Failure::Budget)?;
            require(call.harness.trace_total.0 == elapsed && call.harness.counts.iter().map(|count| count.0).sum::<u64>() == elapsed
                && call.harness.guard.enabled && !call.harness.guard.halted && call.harness.guard.checked_fetches.0 == elapsed
                && call.harness.guard.accepted_fetches.0 == elapsed && call.harness.guard.vector_fetches.0 == 0, Failure::Clock)?;
            require(call.harness.guard.ranges.as_ref().is_some_and(|ranges| ranges.len() == self.ranges.len()
                && ranges.iter().zip(&self.ranges).all(|(actual, &(start, end))| *actual == [start, end]))
                && call.harness.guard.vector_ranges.is_empty() && call.harness.guard.vector_range_fetches.is_empty()
                && call.harness.guard.range_fetches.len() == self.ranges.len()
                && call.harness.counts.len() == self.ranges.len()
                && call.harness.guard.range_fetches.iter().zip(&call.harness.counts).all(|(fetches, count)| fetches == count), Failure::Clock)?;
            if candidate { require(call.before.heap_break.0 == POOL_BASE && call.after.heap_break.0 == POOL_BASE, Failure::Context)?; }
            if let Some(status) = call.status {
                require(matches!(status, TargetStatus::Success | TargetStatus::OutOfMemory), Failure::BackendFailure)?;
                if !matches!(role, CallRole::Allocate | CallRole::Resize | CallRole::Calloc) { require(status == TargetStatus::Success, Failure::BackendFailure)?; }
            }
            Ok(())
        })();
        if validation.is_err() { preserve_failure(&call.before, &call.after)?; }
        case.calls.push(call);
        validation?;
        self.check_watch(case)?;
        Ok(identity)
    }
    fn candidate_call(&mut self, case: &mut Case, role: CallRole, symbol: Symbol, arguments: [u32; 4]) -> Result<CallId, Failure> {
        self.call(case, role, self.inputs.symbol(symbol)?, arguments, true)
    }
    fn check_watch(&mut self, case: &mut Case) -> Result<(), Failure> {
        if let Some(watch) = &self.watch {
            let raw = serde_json::to_vec(&watch.records.borrow().state())?;
            let observed: CodeWatch = match serde_json::from_slice(&raw) {
                Ok(observed) => observed,
                Err(_) => { std::fs::write(self.output.join("code-watch.json"), &raw)?; return Err(Failure::CodeWrite); }
            };
            if !observed.enabled || observed.violation.is_some() || observed.classes.len() != self.ranges.len()
                || !observed.classes.iter().zip(&self.ranges).all(|(class, &(start, end))| class.range == [start, end]
                    && class.kinds.as_slice() == [AccessKind::CpuWrite, AccessKind::HostWrite, AccessKind::ModelBusWrite])
                || observed.views != Views::Observe || observed.ranges.len() != self.ranges.len()
                || !observed.ranges.iter().zip(&self.ranges).all(|(actual, &(start, end))| *actual == [start, end]) {
                std::fs::write(self.output.join("code-watch.json"), &raw)?;
                return Err(Failure::CodeWrite);
            }
            case.code_watch = Some(observed);
        }
        Ok(())
    }
    fn result(case: &Case, id: CallId) -> u32 { case.calls[id.0].after.registers[0] }
    fn unit(case: &Case, id: CallId) -> Result<(), Failure> {
        require(Self::result(case, id) == 0 && case.calls[id.0].status == Some(TargetStatus::Success), Failure::BackendFailure)
    }
    fn allocate(&mut self, case: &mut Case, role: CallRole, layout: Layout) -> Result<Address, Failure> {
        let id = if case.backend == Backend::Stock {
            self.call(case, role, Address(0x89cda4), [layout.size.0, 0, 0, 0], false)?
        } else { self.candidate_call(case, role, Symbol::Allocate, [layout.size.0, layout.alignment.bytes(), 0, 0])? };
        let pointer = Address(Self::result(case, id));
        if case.backend != Backend::Stock {
            require(case.calls[id.0].status == Some(if pointer.0 == 0 { TargetStatus::OutOfMemory } else { TargetStatus::Success }), Failure::BackendFailure)?;
        }
        Ok(pointer)
    }
    fn free(&mut self, case: &mut Case, allocation: Option<Live>) -> Result<(), Failure> {
        let pointer = allocation.map_or(0, |allocation| allocation.pointer.0);
        if case.backend == Backend::Stock {
            self.call(case, CallRole::Free, Address(0x89cdc4), [pointer, 0, 0, 0], false)?;
        } else {
            let (size, alignment) = allocation.map_or((0, 8), |allocation| (allocation.layout.size.0, allocation.layout.alignment.bytes()));
            let id = self.candidate_call(case, CallRole::Free, Symbol::Free, [pointer, size, alignment, 0])?;
            Self::unit(case, id)?;
        }
        Ok(())
    }
    fn inspect(&mut self, role: DataRole, identity: AllocationId, pointer: Address, bytes: Bytes) -> Result<Data, Failure> {
        self.scratch.resize(bytes.0 as usize, 0);
        let reading = read_into(&self.session.board, &self.maps, pointer, &mut self.scratch, &mut self.physical_scratch);
        let data = Data { role, identity, pointer, bytes: PayloadBytes(self.scratch.clone()) };
        if let Err(error) = reading {
            save(&self.output.join("last-data.json"), &(&data, &self.physical_scratch))?;
            return Err(error);
        }
        if !self.scratch.iter().enumerate().all(|(offset, byte)| *byte == if role == DataRole::ZeroedCalloc { 0 } else { pattern(identity, offset) }) {
            save(&self.output.join("last-data.json"), &data)?;
            return Err(Failure::Data);
        }
        Ok(data)
    }
    fn fill(&mut self, identity: AllocationId, allocation: Live) -> Result<(), Failure> {
        self.scratch.resize(allocation.layout.size.0 as usize, 0);
        for (offset, byte) in self.scratch.iter_mut().enumerate() { *byte = pattern(identity, offset); }
        if !self.scratch.is_empty() {
            self.session.board.write_physical(physical(&self.maps, allocation.pointer, self.scratch.len())?, &self.scratch).map_err(execution)?;
        }
        Ok(())
    }
    fn placement(&self, case: &Case, pointer: Address, layout: Layout, except: Option<AllocationId>) -> Result<(), Failure> {
        require(pointer.0 != 0 && pointer.0 % layout.alignment.bytes() == 0
            && Span { start: Address(POOL_BASE), end: Address(POOL_BASE + case.capacity.bytes().0) }.contains(pointer, Bytes(layout.size.0.max(1))), Failure::Footprint)?;
        let end = pointer.0.checked_add(layout.size.0.max(1)).ok_or(Failure::Footprint)?;
        for (identity, allocation) in &self.live {
            if Some(*identity) == except { continue; }
            let peer_end = allocation.pointer.0.checked_add(allocation.layout.size.0.max(1)).ok_or(Failure::Footprint)?;
            require(end <= allocation.pointer.0 || pointer.0 >= peer_end, Failure::Overlap)?;
        }
        Ok(())
    }
    fn footprint(&mut self, case: &mut Case, pointer: Address, layout: Layout) -> Result<(Footprint, Bytes), Failure> {
        if case.backend == Backend::Stock {
            let header = word(&self.session.board, &self.maps, pointer.0.checked_sub(4).ok_or(Failure::Footprint)?)?;
            let bytes = header.0 & !3;
            let observation = Footprint::Stock { pointer, header };
            if !(header.0 & 2 == 0 && bytes >= 16 && bytes % 8 == 0 && bytes.checked_sub(4).is_some_and(|usable| usable >= layout.size.0)
                && pointer.0.checked_sub(8).and_then(|start| start.checked_add(bytes)).is_some_and(|end| end <= POOL_BASE + case.capacity.bytes().0)) {
                save(&self.output.join("last-footprint.json"), &observation)?;
                return Err(Failure::Footprint);
            }
            Ok((observation, Bytes(bytes)))
        } else {
            let id = self.candidate_call(case, CallRole::Footprint, Symbol::Footprint, [pointer.0, layout.size.0, layout.alignment.bytes(), 0])?;
            let bytes = Self::result(case, id);
            require(bytes != 0 && bytes >= layout.size.0 && bytes <= case.capacity.bytes().0, Failure::Footprint)?;
            Ok((Footprint::Candidate { call: id }, Bytes(bytes)))
        }
    }
    fn statistics(&mut self, case: &mut Case, expected_allocated: u32) -> Result<Statistics, Failure> {
        let observation;
        let stats;
        if case.backend == Backend::Stock {
            let heap_break = Address(word(&self.session.board, &self.maps, 0x829a08)?.0);
            let top = Address(word(&self.session.board, &self.maps, 0x89d500)?.0);
            let mut chunks = Vec::new();
            let totals = (|| -> Result<_, Failure> {
                require(heap_break.0 >= POOL_BASE && heap_break.0 <= POOL_BASE + case.capacity.bytes().0, Failure::Statistics)?;
                let mut cursor = POOL_BASE;
                let (mut allocated, mut free, mut largest) = (0u32, 0u32, 0u32);
                if cursor == heap_break.0 {
                    require(!(POOL_BASE..POOL_BASE + case.capacity.bytes().0).contains(&top.0), Failure::Statistics)?;
                } else {
                    require((POOL_BASE..heap_break.0).contains(&top.0), Failure::Statistics)?;
                }
                while cursor < heap_break.0 {
                    require(chunks.len() < case.capacity.bytes().0 as usize / 16, Failure::Statistics)?;
                    let header_address = cursor.checked_add(4).ok_or(Failure::Statistics)?;
                    let header = word(&self.session.board, &self.maps, header_address)?;
                    chunks.push(ChunkHeader { address: Address(cursor), word: header });
                    let bytes = header.0 & !3;
                    require(header.0 & 2 == 0 && bytes >= 16 && bytes % 8 == 0, Failure::Statistics)?;
                    let next = cursor.checked_add(bytes).ok_or(Failure::Statistics)?;
                    require(next <= heap_break.0, Failure::Statistics)?;
                    let is_free = if cursor == top.0 {
                        require(next == heap_break.0, Failure::Statistics)?;
                        true
                    } else {
                        require(next < heap_break.0, Failure::Statistics)?;
                        word(&self.session.board, &self.maps, next + 4)?.0 & 1 == 0
                    };
                    if is_free { free = free.checked_add(bytes).ok_or(Failure::Statistics)?; largest = largest.max(bytes); }
                    else { allocated = allocated.checked_add(bytes).ok_or(Failure::Statistics)?; }
                    cursor = next;
                }
                Ok((allocated, free, largest))
            })();
            let (allocated, free, largest) = match totals {
                Ok(totals) => totals,
                Err(error) => {
                    save(&self.output.join("last-statistics.json"), &Statistics::Stock { heap_break, top, chunks, external_metadata_bytes: Some(STOCK_EXTERNAL_METADATA) })?;
                    return Err(error);
                }
            };
            observation = Statistics::Stock { heap_break, top, chunks, external_metadata_bytes: Some(STOCK_EXTERNAL_METADATA) };
            stats = Stats { allocated_block_bytes: Bytes(allocated), free_block_bytes: Bytes(free), largest_free_block_bytes: Bytes(largest),
                external_metadata_bytes: Some(STOCK_EXTERNAL_METADATA), pool_managed_bytes: Bytes(heap_break.0 - POOL_BASE) };
        } else {
            self.session.board.write_physical(physical(&self.maps, Address(STATS_ADDRESS), 20)?, &[0xa5; 20]).map_err(execution)?;
            let id = self.candidate_call(case, CallRole::Statistics, Symbol::Statistics, [STATS_ADDRESS, 0, 0, 0])?;
            Self::unit(case, id)?;
            let mut words = [Word(0); 5];
            for (index, value) in words.iter_mut().enumerate() { *value = word(&self.session.board, &self.maps, STATS_ADDRESS + 4 * index as u32)?; }
            observation = Statistics::Candidate { call: id, output: Address(STATS_ADDRESS), words };
            stats = Stats { allocated_block_bytes: Bytes(words[0].0), free_block_bytes: Bytes(words[1].0), largest_free_block_bytes: Bytes(words[2].0),
                external_metadata_bytes: Some(Bytes(words[3].0)), pool_managed_bytes: Bytes(words[4].0) };
        }
        if !(stats.allocated_block_bytes.0 == expected_allocated && stats.largest_free_block_bytes.0 <= stats.free_block_bytes.0
            && stats.allocated_block_bytes.0.checked_add(stats.free_block_bytes.0).is_some_and(|bytes| bytes <= stats.pool_managed_bytes.0)
            && stats.pool_managed_bytes.0 <= case.capacity.bytes().0) {
            save(&self.output.join("last-statistics.json"), &observation)?;
            return Err(Failure::Statistics);
        }
        Ok(observation)
    }
    fn allocated(&self) -> Result<u32, Failure> {
        self.live.values().try_fold(0u32, |sum, allocation| sum.checked_add(allocation.footprint.0).ok_or(Failure::Statistics))
    }
    fn old(&self, identity: Option<AllocationId>, layout: Option<Layout>) -> Result<Option<Live>, Failure> {
        match (identity, layout) {
            (None, None) => Ok(None),
            (Some(identity), Some(layout)) => {
                let allocation = *self.live.get(&identity).ok_or(Failure::Tape)?;
                require(allocation.layout == layout, Failure::Tape)?;
                Ok(Some(allocation))
            }
            _ => Err(Failure::Tape),
        }
    }
    fn step(&mut self, case: &mut Case, request: &Request, record: &mut StepRecord) -> Result<(), Failure> {
        let mut measured_footprint = None;
        let (layout, outcome, previous, zeroed, pointer) = match *request {
            Request::Free { identity, layout } => {
                let old = self.old(identity, layout)?;
                if let (Some(identity), Some(old)) = (identity, old) {
                    record.data.push(self.inspect(DataRole::BeforeFree, identity, old.pointer, old.layout.size)?);
                }
                self.free(case, old)?;
                if let Some(identity) = identity { self.live.remove(&identity); }
                record.statistics.push(self.statistics(case, self.allocated()?)?);
                return Ok(());
            }
            Request::Malloc { layout, outcome } => (layout, outcome, None, false, self.allocate(case, CallRole::Allocate, layout)?),
            Request::Calloc { count, element, layout, outcome } => {
                require(count.0.checked_mul(element.0) == Some(layout.size.0), Failure::Tape)?;
                let pointer = if case.backend == Backend::Stock {
                    let id = self.call(case, CallRole::Calloc, CALLOC.start, [count.0, element.0, 0, 0], false)?;
                    Address(Self::result(case, id))
                } else { self.allocate(case, CallRole::Calloc, layout)? };
                record.result = pointer;
                Self::outcome(outcome, pointer)?;
                if pointer.0 != 0 && case.backend != Backend::Stock {
                    self.placement(case, pointer, layout, None)?;
                    let (observation, footprint) = self.footprint(case, pointer, layout)?;
                    record.footprint = Some(observation);
                    measured_footprint = Some(footprint);
                    let id = self.candidate_call(case, CallRole::PayloadZero, Symbol::Zero, [pointer.0, layout.size.0, 0, 0])?;
                    Self::unit(case, id)?;
                }
                (layout, outcome, None, true, pointer)
            }
            Request::Realloc { previous, previous_layout, layout, outcome } => {
                let old = self.old(previous, previous_layout)?;
                if let (Some(identity), Some(old)) = (previous, old) {
                    record.data.push(self.inspect(DataRole::BeforeResize, identity, old.pointer, old.layout.size)?);
                }
                let pointer = if case.backend == Backend::Stock {
                    let id = self.call(case, CallRole::Resize, Address(0x89da3c), [old.map_or(0, |old| old.pointer.0), layout.size.0, 0, 0], false)?;
                    Address(Self::result(case, id))
                } else { self.allocate(case, CallRole::Resize, layout)? };
                record.result = pointer;
                Self::outcome(outcome, pointer)?;
                if pointer.0 != 0 {
                    self.placement(case, pointer, layout, if case.backend == Backend::Stock { previous } else { None })?;
                    if let (Some(identity), Some(old)) = (previous, old) {
                        let prefix = Bytes(old.layout.size.0.min(layout.size.0));
                        if case.backend != Backend::Stock {
                            let (observation, footprint) = self.footprint(case, pointer, layout)?;
                            record.footprint = Some(observation);
                            measured_footprint = Some(footprint);
                            let transient = self.allocated()?.checked_add(footprint.0).ok_or(Failure::Statistics)?;
                            record.statistics.push(self.statistics(case, transient)?);
                            let id = self.candidate_call(case, CallRole::PayloadCopy, Symbol::Copy, [pointer.0, old.pointer.0, prefix.0, 0])?;
                            Self::unit(case, id)?;
                        }
                        record.data.push(self.inspect(DataRole::PreservedResize, identity, pointer, prefix)?);
                        if case.backend != Backend::Stock { self.free(case, Some(old))?; }
                    }
                } else if let (Some(identity), Some(old)) = (previous, old) {
                    record.data.push(self.inspect(DataRole::PreservedResize, identity, old.pointer, old.layout.size)?);
                }
                (layout, outcome, previous, false, pointer)
            }
        };
        record.result = pointer;
        Self::outcome(outcome, pointer)?;
        if let AllocationOutcome::Success { identity } = outcome {
            require(previous.is_none_or(|previous| previous == identity), Failure::Tape)?;
            self.placement(case, pointer, layout, previous)?;
            require(previous.is_some() || !self.live.contains_key(&identity), Failure::Tape)?;
            if zeroed { record.data.push(self.inspect(DataRole::ZeroedCalloc, identity, pointer, layout.size)?); }
            let footprint = if let Some(footprint) = measured_footprint { footprint } else {
                let (observation, footprint) = self.footprint(case, pointer, layout)?;
                record.footprint = Some(observation);
                footprint
            };
            let allocation = Live { pointer, layout, footprint };
            self.live.insert(identity, allocation);
            self.fill(identity, allocation)?;
        }
        record.statistics.push(self.statistics(case, self.allocated()?)?);
        Ok(())
    }
    fn outcome(outcome: AllocationOutcome, pointer: Address) -> Result<(), Failure> {
        require(matches!((outcome, pointer.0), (AllocationOutcome::Failure, 0))
            || matches!(outcome, AllocationOutcome::Success { .. }) && pointer.0 != 0, Failure::Divergence)
    }
}

fn bind(runtime: &mut Runtime<'_>, case: &mut Case, span: Span, expected: &[u8]) -> Result<(), Failure> {
    require(span.start.0 < span.end.0 && expected.len() == span.bytes().0 as usize, Failure::CodeExtent)?;
    runtime.scratch.resize(expected.len(), 0);
    read_into(&runtime.session.board, &runtime.maps, span.start, &mut runtime.scratch, &mut runtime.physical_scratch)?;
    let expected_digest = Digest(sha256(expected));
    let before = Digest(sha256(&runtime.scratch));
    case.code.push(CodeBinding { span, expected: expected_digest, before, after: None });
    if runtime.scratch != expected {
        let offset = runtime.scratch.iter().zip(expected).position(|(actual, expected)| actual != expected).ok_or(Failure::CodeExtent)? & !3;
        let actual = runtime.scratch.get(offset..offset + 4).ok_or(Failure::CodeExtent)?;
        let wanted = expected.get(offset..offset + 4).ok_or(Failure::CodeExtent)?;
        return Err(Failure::CodeWord { address: Address(span.start.0 + offset as u32), expected: Word(u32::from_le_bytes(wanted.try_into().map_err(|_| Failure::CodeExtent)?)),
            actual: Word(u32::from_le_bytes(actual.try_into().map_err(|_| Failure::CodeExtent)?)) });
    }
    runtime.ranges.push((u64::from(span.start.0), u64::from(span.end.0)));
    Ok(())
}
fn prepare<'a>(inputs: &'a Inputs, tape: &Tape, output: &'a Path, case: &mut Case) -> Result<Runtime<'a>, Failure> {
    require(Digest(sha256(&inputs.run)) == inputs.bindings.run && Digest(sha256(&inputs.update)) == inputs.bindings.update
        && Digest(sha256(&inputs.boot_rom)) == inputs.bindings.boot_rom && Digest(sha256(&inputs.seed)) == inputs.bindings.seed
        && Digest(sha256(&inputs.payload)) == inputs.bindings.payload, Failure::InputIdentity)?;
    require(inputs.bindings.seed.0 == "03529deb1a01a1f6f488ad7b48e63bf8271deca76268b2c10616588f9cc109f4", Failure::InputIdentity)?;
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&inputs.run).map_err(execution)?;
    let maps = run::memory_maps(&document).map_err(execution)?;
    let mut session = run::open(&document, &ValidatedUpdate(inputs.update.clone()), &inputs.boot_rom).map_err(execution)?;
    session.board.set_execution_policy(document.execution_policy.into()).map_err(execution)?;
    let snapshot = session.import_snapshot(&inputs.seed).map_err(execution)?;
    session.restore(&snapshot).map_err(execution)?;
    let seed = frame(&session.board, &maps)?;
    save(&output.join("seed-frame.json"), &seed)?;
    require(seed.registers[15] == 0x89cde4 && seed.registers[0] == REENT && seed.registers[1] == 384
        && seed.counts == [Instructions(26_267_839), Instructions(3_215_110)] && seed.cpsr.0 & 0xff == 0x53
        && seed.root.0 == 0x4004dae0 && seed.task_record.0 == 0x40060ecc && seed.kernel_context.0 == 0
        && seed.identity.0 == 0x4003 && seed.owner.0 == 0 && seed.depth.0 == 0 && seed.heap_break.0 == POOL_BASE, Failure::Context)?;
    require(word(&session.board, &maps, 0x89c970)?.0 == REENT && word(&session.board, &maps, seed.root.0 + 0x16c)?.0 == 0x4118c
        && word(&session.board, &maps, seed.root.0 + 0x1618)?.0 == 0x412bc, Failure::Context)?;
    let peer_identity = session.board.cpu(1).map_err(execution)?.rprofile_identity(None).map_err(execution)?;
    require(peer_identity == fujimod_xm1::run::CoreIdentities::Xm1.words()[1], Failure::Context)?;
    let mut runtime = Runtime { inputs, output, session, maps, guard: Guard::default(), watch: None, ranges: Vec::new(), live: BTreeMap::new(),
        scratch: Vec::with_capacity(tape.maximum_request().0 as usize), physical_scratch: Vec::with_capacity(tape.maximum_request().0 as usize), peer_identity,
        quantum: ProcessorQuantum(std::num::NonZeroUsize::new(document.execution.quantum).ok_or(Failure::Context)?) };
    let bound = word(&runtime.session.board, &runtime.maps, 0x72b130)?;
    require(bound.0 == Capacity::Original.bound_word(), Failure::CodeWord { address: Address(0x72b130), expected: Word(Capacity::Original.bound_word()), actual: bound })?;
    if case.capacity == Capacity::Recipe {
        runtime.session.board.write_physical(physical(&runtime.maps, Address(0x72b130), 4)?, &case.capacity.bound_word().to_le_bytes()).map_err(execution)?;
    }
    if tape.steps().iter().any(|step| matches!(step.request, Request::Calloc { .. })) {
        let bytes = inputs.canonical(CALLOC)?;
        runtime.session.board.write_physical(physical(&runtime.maps, CALLOC.start, bytes.len())?, &bytes).map_err(execution)?;
        save(&output.join("isolated-calloc-fixture.json"), &(CALLOC, Digest(sha256(&bytes))))?;
    }
    let image_start = inputs.symbol(Symbol::ImageStart)?.0;
    let image_end = inputs.symbol(Symbol::ImageEnd)?.0;
    let bss_start = inputs.symbol(Symbol::BssStart)?.0;
    let bss_end = inputs.symbol(Symbol::BssEnd)?.0;
    let payload_end = image_start.checked_add(u32::try_from(inputs.payload.len()).map_err(|_| Failure::Symbols)?).ok_or(Failure::Symbols)?;
    require(image_start == IMAGE_BASE && payload_end > image_start && payload_end <= bss_start && bss_start <= bss_end
        && bss_end == image_end && image_end <= IMAGE_LIMIT, Failure::Symbols)?;
    runtime.session.board.write_physical(physical(&runtime.maps, Address(image_start), inputs.payload.len())?, &inputs.payload).map_err(execution)?;
    if bss_end > bss_start {
        runtime.scratch.resize((bss_end - bss_start) as usize, 0);
        runtime.scratch.fill(0);
        runtime.session.board.write_physical(physical(&runtime.maps, Address(bss_start), runtime.scratch.len())?, &runtime.scratch).map_err(execution)?;
        runtime.physical_scratch.clear();
        read_into(&runtime.session.board, &runtime.maps, Address(bss_start), &mut runtime.scratch, &mut runtime.physical_scratch)?;
        require(runtime.scratch.iter().all(|byte| *byte == 0), Failure::Data)?;
    }
    runtime.scratch.resize(inputs.payload.len(), 0);
    read_into(&runtime.session.board, &runtime.maps, Address(image_start), &mut runtime.scratch, &mut runtime.physical_scratch)?;
    require(runtime.scratch == inputs.payload, Failure::Data)?;
    let text = inputs.payload_text()?;
    require(text.start.0 >= image_start && text.end.0 <= payload_end, Failure::Symbols)?;
    let spans = code_spans(inputs, tape)?;
    for &span in &spans {
        let expected = expected_code(inputs, span, case.capacity)?;
        bind(&mut runtime, case, span, &expected)?;
    }
    require(runtime.ranges.iter().map(|&(start, end)| [start, end]).collect::<Vec<_>>() == merged_watch_ranges(&spans), Failure::CodeExtent)?;
    let classes = runtime.ranges.iter().map(|&(start, end)| WatchRange { start, end, kinds: WatchKinds::WRITES }).collect();
    runtime.watch = Some(runtime.session.board.memory_watch_begin_classified(classes, 1024, runtime.maps.clone(), ViewPolicy::Observe).map_err(execution)?);
    runtime.check_watch(case)?;
    runtime.guard.arm(&runtime.session.board).map_err(execution)?;
    Ok(runtime)
}

pub fn run_case(inputs: &Inputs, tape: &Tape, backend: Backend, capacity: Capacity, output: &Path) -> Case {
    let mut case = Case { schema: 1, backend, capacity, bindings: inputs.bindings.clone(), complete: false, error: None,
        code: Vec::new(), code_watch: None, initial_statistics: None, calls: Vec::new(), steps: Vec::new(), final_data: Vec::new(), final_statistics: None };
    let result = (|| -> Result<(), Failure> {
        let mut runtime = prepare(inputs, tape, output, &mut case)?;
        let replay = (|| -> Result<(), Failure> {
            if let Some(wire) = backend.wire() {
                let id = runtime.candidate_call(&mut case, CallRole::Initialize, Symbol::Init, [POOL_BASE, capacity.bytes().0, wire, 0])?;
                Runtime::unit(&case, id)?;
            }
            case.initial_statistics = Some(runtime.statistics(&mut case, 0)?);
            for (index, step) in tape.steps().iter().enumerate() {
                require(step.core.0 == 0 && step.context.domain.0 == 0 && step.context.mode == 0x13 && step.context.exception_depth == 0, Failure::Tape)?;
                let first_call = case.calls.len();
                let mut record = StepRecord { index: StepIndex(index), sequence: step.sequence, calls: Vec::new(), result: Address(0),
                    footprint: None, statistics: Vec::new(), data: Vec::new() };
                let result = runtime.step(&mut case, &step.request, &mut record);
                record.calls = (first_call..case.calls.len()).map(CallId).collect();
                case.steps.push(record);
                result?;
            }
            require(case.steps.len() == tape.steps().len() && runtime.live.len() == tape.final_live().len(), Failure::Incomplete)?;
            for (&identity, &layout) in tape.final_live() {
                let allocation = *runtime.live.get(&identity).ok_or(Failure::Incomplete)?;
                require(allocation.layout == layout, Failure::Incomplete)?;
                let (_, observed_footprint) = runtime.footprint(&mut case, allocation.pointer, layout)?;
                require(observed_footprint == allocation.footprint, Failure::Footprint)?;
                case.final_data.push(runtime.inspect(DataRole::FinalLive, identity, allocation.pointer, layout.size)?);
            }
            case.final_statistics = Some(runtime.statistics(&mut case, runtime.allocated()?)?);
            Ok(())
        })();
        let watch_result = runtime.check_watch(&mut case);
        let disarm_watch = if let Some(watch) = &mut runtime.watch { runtime.session.board.memory_watch_end(watch).map_err(execution) } else { Err(Failure::CodeWrite) };
        let disarm_guard = runtime.guard.disarm(&runtime.session.board).map_err(execution);
        let code_result = (|| -> Result<(), Failure> {
            for binding in &mut case.code {
                runtime.scratch.resize(binding.span.bytes().0 as usize, 0);
                read_into(&runtime.session.board, &runtime.maps, binding.span.start, &mut runtime.scratch, &mut runtime.physical_scratch)?;
                let after = Digest(sha256(&runtime.scratch));
                let matches = after == binding.expected;
                binding.after = Some(after);
                require(matches, Failure::CodeWrite)?;
            }
            Ok(())
        })();
        replay?;
        watch_result?;
        disarm_watch?;
        disarm_guard?;
        code_result?;
        Ok(())
    })();
    match result { Ok(()) => case.complete = true, Err(error) => case.error = Some(error) }
    case
}
