use super::*;
use crate::provenance::{self, Form};

const HEAD: (u64, u64) = (0, 0x24);
const TAIL: (u64, u64) = (0x24, 0x40);
const CODE: [(u32, u32); 2] = [(0x40020, 0x40100), (0x427b8, 0x427d4)];
const SITES: [u32; 7] = [0x4003c, 0x40048, 0x40054, 0x40060, 0x4006c, 0x40078, 0x40084];
const BEFORE: [u32; 7] = [0x4c, 0x64, 0x50, 0x58, 0x60, 0xc0, 0x90];
const HANDLERS: [u32; 7] = [0x40100, 0x401b4, 0x40128, 0x40148, 0x40168, 0x40204, 0x40184];
const INTERLEAVED_SITE: u32 = 0x40040;
const INTERLEAVED_ORIGINAL: u32 = 0xe3a03028;
const INTERLEAVED_HELPER: [u32; 7] = [
    0xe3a03024, 0xe59f4008, 0xe7834006, 0xe3a03028, 0xe59ff000, 0xdeadbe00, 0x40044,
];

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InterleavedTail {
    site: Guest,
    instruction: Word,
    target: Guest,
}

impl InterleavedTail {
    pub(super) fn validate(&self, image: (Guest, Guest)) -> Result<(), String> {
        let displacement = (i64::from(self.instruction.0 & 0x00ff_ffff) << 40) >> 38;
        require(self.site == Guest(INTERLEAVED_SITE) && self.instruction.0 & 0xff00_0000 == 0xea00_0000
            && i64::from(self.site.0) + 8 + displacement == i64::from(self.target.0)
            && self.target.0 % 4 == 0 && image.0 .0 <= self.target.0
            && self.target.0.checked_add(28).is_some_and(|end| end <= image.1 .0),
            "interleaved-tail hook is not the fixed branch into its owned helper")
    }

    pub(super) fn bind(&self, base: Guest, bytes: &[u8]) -> Result<(), String> {
        let offset = self.target.0.checked_sub(base.0).ok_or("interleaved-tail helper precedes owned image")? as usize;
        let helper = bytes.get(offset..offset + 28).ok_or("interleaved-tail helper leaves owned image")?;
        require(helper.chunks_exact(4).zip(INTERLEAVED_HELPER).all(|(actual, expected)| actual == expected.to_le_bytes()),
            "interleaved-tail owned helper bytes differ")
    }

    fn canonical_word(&self, site: u32, instruction: u32) -> Result<u32, String> {
        if site != self.site.0 { return Ok(instruction); }
        require(instruction == self.instruction.0, "interleaved-tail branch word differs")?;
        Ok(INTERLEAVED_ORIGINAL)
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CompletionZero {
    pub site: Guest,
    pub instruction: Word,
    pub literal: Guest,
}

impl CompletionZero {
    pub(super) fn validate(&self, image: (Guest, Guest)) -> Result<(), String> {
        let inside = |site: Guest| site.0 % 4 == 0 && image.0 .0 <= site.0 && site.0 < image.1 .0;
        require(inside(self.site) && inside(self.literal) && word_store(self.instruction.0),
            "completion-zero instruction or literal is outside the owned image")
    }

    pub(super) fn bind(&self, base: Guest, bytes: &[u8]) -> Result<(), String> {
        let at = |site: u32| -> Option<u32> {
            let offset = usize::try_from(site.checked_sub(base.0)?).ok()?;
            bytes.get(offset..offset.checked_add(4)?).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        };
        let mut candidates = provenance::references(base, bytes, &|value| value.0 == CELL_GUEST as u32)
            .into_iter().filter(|reference| reference.form == Form::Code)
            .filter_map(|reference| {
                let instruction = at(reference.at.0)?;
                if !word_store(instruction) { return None; }
                let base_register = (instruction >> 16) & 15;
                (1..=3).find_map(|distance| {
                    let load_site = reference.at.0.checked_sub(distance * 4)?;
                    let load = at(load_site)?;
                    if load & 0xffff_0000 != 0xe59f_0000 || (load >> 12) & 15 != base_register { return None; }
                    let literal = load_site.checked_add(8)?.checked_add(load & 0xfff)?;
                    (at(literal) == Some(CELL_GUEST as u32)).then_some((reference.at, Word(instruction), Guest(literal)))
                })
            });
        let selected = candidates.next();
        require(selected == Some((self.site, self.instruction, self.literal))
            && candidates.all(|candidate| Some(candidate) == selected), "owned completion-zero code proof differs")
    }
}

fn word_store(word: u32) -> bool {
    word & 0xfff0_0fff == 0xe580_0000 && (word >> 16) & 15 <= 12 && (word >> 12) & 15 <= 12
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CodeFact { pub span: (Guest, Guest), pub sha256: Sha256Digest }

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Step { pub site: Guest, pub slot: Guest, pub before: Word, pub value: Word }

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Facts {
    pub entry: Guest,
    pub installed: Guest,
    pub bootstrap: [Word; 16],
    pub code: [CodeFact; 2],
    pub steps: [Step; 7],
}

impl Facts {
    pub(super) fn validate(&self, startup: &ComposedStartup) -> Result<(), String> {
        require(self.entry == startup.handoff_target && self.entry == Guest(0x40020) && self.installed == Guest(0x40088)
            && self.bootstrap[8] == Word(startup.entry.0), "kernel-vector entry or bootstrap reset differs")?;
        for (fact, (start, end)) in self.code.iter().zip(CODE) {
            require(fact.span == (Guest(start), Guest(end)), "kernel-vector code extent differs")?;
        }
        for (index, step) in self.steps.iter().enumerate() {
            require(step.site == Guest(SITES[index]) && step.slot == Guest(0x24 + index as u32 * 4)
                && step.before == Word(BEFORE[index]) && step.value == Word(HANDLERS[index])
                && self.bootstrap[9 + index] == step.before, "kernel-vector source step differs")?;
        }
        Ok(())
    }

    fn expected(&self, matched: usize) -> [u8; 64] {
        let mut bytes = [0; 64];
        for (index, word) in self.bootstrap.iter().enumerate() {
            let value = if (9..9 + matched).contains(&index) { self.steps[index - 9].value.0 } else { word.0 };
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn validate_code(&self, code: &[CodeViews; 2], hook: Option<&InterleavedTail>) -> Result<(), String> {
        let at = |address: u32| -> Result<u32, String> {
            for region in code {
                let (start, end) = region.span;
                if (start.0..end.0).contains(&address) {
                    let offset = (address - start.0) as usize;
                    let bytes = region.bytes.physical.get(offset..offset + 4).ok_or("kernel-vector instruction outside code")?;
                    let instruction = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    return hook.map_or(Ok(instruction), |hook| hook.canonical_word(address, instruction));
                }
            }
            Err("kernel-vector instruction outside code".into())
        };
        for (index, instruction) in [0xe1a05000, 0xeb0009e3, 0xe3500000, 0x03a06000, 0x159f60a4].into_iter().enumerate() {
            require(at(0x40020 + index as u32 * 4)? == instruction, "kernel-vector selector call differs")?;
        }
        require(at(0x400dc)? == 0xffff0000 && at(0x40088)? == 0xe1a00005, "kernel-vector selector literal or boundary differs")?;
        for (index, step) in self.steps.iter().enumerate() {
            let load_site = step.site.0 - 4;
            let literal = 0x400e0 + index as u32 * 4;
            require(at(step.site.0 - 8)? == (0xe3a03000 | step.slot.0)
                && at(load_site)? == (0xe59f4000 | (literal - load_site - 8))
                && at(step.site.0)? == 0xe7834006 && at(literal)? == step.value.0,
                "kernel-vector store or handler literal differs")?;
        }
        for (index, instruction) in [0xee121e10, 0xe3a02901, 0xe2822003, 0xe1510002, 0x03a00000, 0x13a00001, 0xe12fff1e]
            .into_iter().enumerate() {
            require(at(0x427b8 + index as u32 * 4)? == instruction, "kernel-vector CP14 selector differs")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum HistoryEpoch { Loaded, Scoped }
#[derive(Clone, Copy, Serialize)]
#[serde(transparent)]
struct RowCursor(usize);
#[derive(Clone, Copy, Serialize)]
pub(super) struct HistoryRef { epoch: HistoryEpoch, index: RowCursor }
#[derive(Clone, Copy, Serialize)]
#[serde(transparent)]
struct StopRef(usize);
#[derive(Serialize)]
#[serde(transparent)]
struct Matched(u8);

#[derive(Serialize)]
#[serde(transparent)]
struct RamChoice(u8);
#[derive(Serialize)]
struct DecoderSnapshot { choices: [RamChoice; 1], pending: [(); 1], failure: () }
#[derive(Serialize)]
pub(super) struct VectorSnapshot { matched: Matched, vectors: Views, decoder: DecoderSnapshot, tcm: TcmWords }
#[derive(Serialize)]
struct CodeViews { span: (Guest, Guest), bytes: Views }
#[derive(Serialize)]
pub(super) struct KernelSnapshot {
    #[serde(flatten)]
    vectors: VectorSnapshot,
    code: [CodeViews; 2],
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "kebab-case")]
enum State { #[default] NotBound, Installing, Installed }

#[derive(Default)]
pub(super) struct History {
    state: State,
    pub zero: Option<HistoryRef>,
    pub(super) loaded_cursor: usize,
    scoped_cursor: usize,
    bound_at: Option<Instructions>,
    binding: Option<KernelSnapshot>,
    events: Vec<HistoryRef>,
    record2_handoff: Option<VectorSnapshot>,
    installed_stop: Option<StopRef>,
    installed_readback: Option<KernelSnapshot>,
    terminal_readback: Option<KernelSnapshot>,
}

#[derive(Serialize)]
pub(super) struct Proof<'a> {
    state: State,
    binding: &'a Option<KernelSnapshot>,
    events: &'a [HistoryRef],
    record2_handoff: &'a Option<VectorSnapshot>,
    installed_stop: Option<StopRef>,
    installed_readback: &'a Option<KernelSnapshot>,
    terminal_readback: &'a Option<KernelSnapshot>,
}

impl History {
    pub(super) fn proof(&self) -> Proof<'_> {
        Proof { state: self.state, binding: &self.binding, events: &self.events, record2_handoff: &self.record2_handoff,
            installed_stop: self.installed_stop, installed_readback: &self.installed_readback, terminal_readback: &self.terminal_readback }
    }
    pub(super) fn handed_off(&self) -> bool { !matches!(self.state, State::NotBound) }
    pub(super) fn installed(&self) -> bool { matches!(self.state, State::Installed) }

    fn consume(&mut self, epoch: HistoryEpoch, rows: &[GuardEvent], startup: &ComposedStartup, scenario: Scenario) -> Result<(), String> {
        let cursor = match epoch { HistoryEpoch::Loaded => self.loaded_cursor, HistoryEpoch::Scoped => self.scoped_cursor };
        require(rows.len() >= cursor && rows.len() <= WATCH_LIMIT, "kernel-vector history cursor or bound differs")?;
        for (index, event) in rows.iter().enumerate().skip(cursor) {
            let reference = HistoryRef { epoch, index: RowCursor(index) };
            let physical = event.physical.ok_or("composed event has no physical location")?.0;
            require(event.size > 0, "composed event has an empty span")?;
            let end = physical.checked_add(event.size).ok_or("composed event span overflows")?;
            let overlaps = |span: (u64, u64)| physical < span.1 && span.0 < end;
            if overlaps((CELL, CELL + 4)) {
                let zero = event.source == GuardSource::CpuWrite && event.core == Some(WitnessCore::Core0)
                    && event.pc == Some(startup.completion_zero.site) && physical == CELL && event.size == 4
                    && event.byte_order == Some(GuardOrder::Little)
                    && event.value == Some(0) && event.before.as_deref() == Some(&[0, 0, 0, 0][..]);
                if zero {
                    require(self.zero.is_none() && !self.handed_off(), "first-stage completion zero repeats or follows handoff")?;
                    self.zero = Some(reference);
                } else {
                    let expected = if scenario == Scenario::NonOne { 0x400a0000 } else { 1 };
                    require(matches!(epoch, HistoryEpoch::Scoped) && self.installed()
                        && event.source == GuardSource::CpuWrite && event.core == Some(WitnessCore::Core0)
                        && event.pc == Some(Guest(STORE)) && physical == CELL && event.size == 4
                        && event.byte_order == Some(GuardOrder::Little)
                        && event.value == Some(expected) && event.before.as_deref() == Some(&[0, 0, 0, 0][..]),
                        "unexpected completion-cell write in composed history")?;
                }
            } else if overlaps(TAIL) {
                require(matches!(self.state, State::Installing), "kernel-vector write outside installation")?;
                let step = startup.kernel_vector_install.steps.get(self.events.len()).ok_or("kernel-vector installation has an extra write")?;
                require(event.source == GuardSource::CpuWrite && event.core == Some(WitnessCore::Core0)
                    && event.pc == Some(step.site) && event.address == Some(Address(u64::from(step.slot.0)))
                    && physical == u64::from(step.slot.0) && event.size == 4 && event.value == Some(u64::from(step.value.0))
                    && event.byte_order == Some(GuardOrder::Little)
                    && event.before.as_deref() == Some(&step.before.0.to_le_bytes()[..])
                    && event.instructions.zip(self.bound_at).is_some_and(|(at, bound)| at.0 > bound.0),
                    "kernel-vector write differs from its next source step")?;
                self.events.push(reference);
            } else if startup.kernel_vector_install.code.iter().any(|code| overlaps((u64::from(code.span.0 .0), u64::from(code.span.1 .0)))) {
                let protected = [FIRST_VECTORS, ZERO, FIRST_REPLACED,
                    (u64::from(startup.image.0 .0), u64::from(startup.image.1 .0))]
                    .into_iter().chain(SECOND_REPLACED).chain(SECOND_POOLS).chain([REPLACED, ENTRY_POOL]);
                require(!protected.into_iter().any(overlaps)
                    && !(scenario == Scenario::Delayed && overlaps(VENEER))
                    && !(matches!(epoch, HistoryEpoch::Scoped)
                        && [RECORD, LOADER_B, LOADER_A_COMPLETION, LOADER_A_LITERAL].into_iter().any(overlaps)),
                    "kernel staging transfer overlaps another protected class")?;
                let writer = match event.source {
                    GuardSource::CpuWrite => event.core == Some(WitnessCore::Core0) && event.pc.is_some()
                        && event.instructions.is_some() && event.address.is_some() && event.size <= 16
                        && event.byte_order == Some(GuardOrder::Little),
                    GuardSource::ModelBusWrite => event.core.is_none() && event.pc.is_none()
                        && event.instructions.is_none() && event.address.is_some()
                        && event.value.is_none() && event.before.is_none() && event.byte_order.is_none(),
                    _ => false,
                };
                require(!self.handed_off() && writer, "kernel-vector code changed after binding or outside staging")?;
            } else {
                return Err("unexpected guarded access in composed history".into());
            }
            match epoch { HistoryEpoch::Loaded => self.loaded_cursor = index + 1, HistoryEpoch::Scoped => self.scoped_cursor = index + 1 }
        }
        Ok(())
    }
}

impl Runner<'_> {
    pub(super) fn refresh_vectors(&mut self) -> Result<Option<ScopedView>, String> {
        if self.epoch == Epoch::Scoped {
            let view = self.scoped_view()?.ok_or("scoped history is absent")?;
            if let Some(startup) = &self.contract.startup {
                self.vectors.consume(HistoryEpoch::Scoped, &view.records, startup, self.contract.scenario)?;
            }
            return Ok(Some(view));
        }
        if self.epoch == Epoch::Loaded {
            if let Some(startup) = &self.contract.startup {
                let state = self.classified.as_ref().ok_or("loaded history is absent")?.records.borrow().state();
                let view: ClassifiedView = serde_json::from_value(state).map_err(|error| error.to_string())?;
                require(view.violation.is_none(), "loaded history recorded a watch violation")?;
                require(!view.records.iter().any(|row| self.contract.composed_event_forbidden(row)), "composed immutable access observed")?;
                self.vectors.consume(HistoryEpoch::Loaded, &view.records, startup, self.contract.scenario)?;
            }
        }
        Ok(None)
    }

    fn vector_snapshot(&self, machine: &Machine) -> Result<VectorSnapshot, String> {
        let startup = self.contract.startup.as_ref().ok_or("kernel-vector contract absent")?;
        let observed: BoardState = serde_json::from_value(machine.session.board.state()).map_err(|error| error.to_string())?;
        require(observed.memory_decoders.choices == [1] && observed.memory_decoders.pending == [None]
            && observed.memory_decoders.failure.is_none(), "kernel-vector decoder is not committed RAM")?;
        let tcm = tcm_words(machine)?;
        tcm_disjoint_views(&self.committed, &tcm, &self.guarded())?;
        let mut vectors = self.views(machine, 0, FIRST_VECTORS.0, FIRST_VECTORS.1 as usize)?;
        self.bind_composed_views(machine, FIRST_VECTORS.0, &mut vectors)?;
        let matched = self.vectors.events.len();
        let expected = startup.kernel_vector_install.expected(matched);
        require(vectors.physical.as_slice() == expected.as_slice(), "kernel-vector views differ from the observed installation prefix")?;
        Ok(VectorSnapshot { matched: Matched(matched as u8), vectors, decoder: DecoderSnapshot { choices: [RamChoice(1)], pending: [()], failure: () }, tcm })
    }

    fn kernel_snapshot(&self, machine: &Machine) -> Result<KernelSnapshot, String> {
        let startup = self.contract.startup.as_ref().ok_or("kernel-vector contract absent")?;
        let read = |fact: &CodeFact| -> Result<CodeViews, String> {
            let (start, end) = fact.span;
            let mut bytes = self.views(machine, 0, u64::from(start.0), (end.0 - start.0) as usize)?;
            self.bind_composed_views(machine, u64::from(start.0), &mut bytes)?;
            let digest = if let Some(hook) = startup.interleaved_tail.as_ref().filter(|hook| (start.0..end.0).contains(&hook.site.0)) {
                let mut canonical = [0; (CODE[0].1 - CODE[0].0) as usize];
                require(bytes.physical.len() == canonical.len(), "interleaved-tail source extent differs")?;
                canonical.copy_from_slice(&bytes.physical);
                let offset = (hook.site.0 - start.0) as usize;
                let actual = u32::from_le_bytes(canonical[offset..offset + 4].try_into().map_err(|_| "interleaved-tail branch width")?);
                canonical[offset..offset + 4].copy_from_slice(&hook.canonical_word(hook.site.0, actual)?.to_le_bytes());
                Sha256Digest::of(&canonical)
            } else {
                Sha256Digest::of(&bytes.physical)
            };
            require(digest == fact.sha256, "kernel-vector source code identity differs")?;
            Ok(CodeViews { span: fact.span, bytes })
        };
        let code = [read(&startup.kernel_vector_install.code[0])?, read(&startup.kernel_vector_install.code[1])?];
        startup.kernel_vector_install.validate_code(&code, startup.interleaved_tail.as_ref())?;
        Ok(KernelSnapshot { vectors: self.vector_snapshot(machine)?, code })
    }

    pub(super) fn bind_kernel(&mut self, machine: &Machine, stop: usize) -> Result<(), String> {
        require(!self.vectors.handed_off() && self.vectors.zero.is_some(), "kernel-vector binding lacks the first-stage zero or repeats handoff")?;
        let snapshot = self.kernel_snapshot(machine)?;
        require(snapshot.vectors.matched.0 == 0, "kernel-vector installation precedes handoff")?;
        self.vectors.bound_at = Some(self.stops[stop].instructions);
        self.vectors.binding = Some(snapshot);
        self.vectors.state = State::Installing;
        Ok(())
    }

    pub(super) fn snapshot_record2_vectors(&mut self, machine: &Machine) -> Result<(), String> {
        if self.contract.startup.is_some() {
            self.vectors.record2_handoff = Some(self.vector_snapshot(machine)?);
        }
        Ok(())
    }

    pub(super) fn installed_kernel(&mut self, machine: &Machine, stop: usize) -> Result<(), String> {
        require(matches!(self.vectors.state, State::Installing) && self.vectors.events.len() == 7,
            "kernel-vector boundary lacks the complete installation")?;
        let snapshot = self.kernel_snapshot(machine)?;
        let context = &self.stops[stop];
        require(context.registers[5].0 == ARGUMENT && context.registers[6].0 == 0
            && context.cpsr.0 & 0x0100_02ff == 0xd3, "kernel-vector installed context differs")?;
        self.vectors.installed_stop = Some(StopRef(stop));
        self.vectors.installed_readback = Some(snapshot);
        self.vectors.state = State::Installed;
        Ok(())
    }

    pub(super) fn terminal_vectors(&mut self, machine: &Machine) -> Result<(), String> {
        if self.contract.startup.is_some() {
            require(self.vectors.installed(), "composed endpoint lacks installed kernel vectors")?;
            self.vectors.terminal_readback = Some(self.kernel_snapshot(machine)?);
        }
        Ok(())
    }
}

pub(super) fn permanent_vectors() -> (u64, u64) { HEAD }
