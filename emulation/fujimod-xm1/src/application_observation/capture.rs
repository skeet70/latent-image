//! The live application recorder `composed-startup` drives: samples at every application, queue and target stop,
//! the task1 stack scope, readiness samples and, after the boundary, the route with settled inputs.
use super::checks::{
    committed, Kind, ADAPTER_ENTRY, COPY_CALL, COPY_RETURN, KERNEL_RECEIVE, PAYLOAD_LIMIT, PRESS, QUEUE_EXIT, QUEUE_ROOTS,
    RECEIVED, RELEASE_CALL, RELEASE_RETURN, TASK1_RESET,
};
use super::readiness::{Due, Search, CHAIN, IMAGE5_PUBLICATION, PERIOD, QUEUE_ENTRY};
use super::{
    hex_word, ApplicationReceipt, ApplicationWrite, ByteView, CodeObservation, CoreIdentity, DynamicTarget, EvidenceWriter, GraphicsState,
    InputIndex, InputPurpose, Menu, Publication, RawView, ReadinessIndex, ReadinessKind, ReadinessSample, Registers, RenderViews, RouteInput,
    Sample, SampleIndex, ScopeObservation, SideFile, SiteRef, Task1, TaskTableViews, VisibleCapture, ALIAS, EVIDENCE, TASK_ROW_BYTES,
    TASK_ROW_LIMIT, VIEW_WORDS, view_word,
};
use crate::composed_catalog::{
    physical_address as physical, Application, CodeLabel, CodeWord, Core, Domain, Instructions, Pc, Role, Sha256, QUEUE_SITES,
};
use crate::composed_receipt::{ByteRange, MenuObservation, WatchRow};
use crate::guest::RouteKey;
use crate::machine::{Machine, RunOutcome};
use crate::provenance::Guest;
use crate::render::Probe;
use crate::run::Address;
use crate::trace::{Hit, TraceStackScopeConfig};
use fujimod_emulator::session::Session;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

const STACK_WORDS: usize = 16;
const SETTLE_LIMIT: u64 = 90_000_000;
const CONTROLLER: u32 = 0x944580;
const CONTROLLER_WORDS: usize = 20;
/// Controller record, controller state in two views, queue root cells and `0x7cad9c`.
const STATE_VIEWS: [(u32, usize); 5] = [(CONTROLLER, CONTROLLER_WORDS), (0x1467588, 80), (0x14676c8, 7), (0x96bd20, 2), (0x7cad9c, 1)];
const ROOT_SLOTS: [u32; 3] = [0xc8, 0xf8, 0x1618];
const ERASED_SETTINGS: (u32, u32, u32) = (69, 52, 12);
const SHOOTING: (u32, u32, u32) = (1, 5, 2);
const TASK_ROW_WORDS: usize = (TASK_ROW_BYTES / 4) as usize;
/// A creation record through its active word at `+36`.
const DESCRIPTOR_WORDS: usize = 10;

#[derive(Clone, Copy)]
enum Armed {
    Site(usize),
    Queue(usize),
    Target,
}

/// An open queue1 receive on task1's stack, keyed by the SP at the queue entry.
struct Receive {
    core: Core,
    sp: u32,
}

/// Rows of the classified watch, counted at publication.
pub type WatchRows<'r> = &'r dyn Fn() -> Result<u64, String>;

pub struct Recorder<'a> {
    application: &'a Application,
    output: &'a Path,
    armed: BTreeMap<u32, Armed>,
    writer: Option<EvidenceWriter>,
    samples: u64,
    readiness: u64,
    task1: Option<Task1>,
    scope_sites: Vec<Guest>,
    skips: u64,
    targets: Vec<DynamicTarget>,
    receives: Vec<Receive>,
    search: Search,
    publication: Option<Publication>,
    publication_code: Vec<(CodeLabel, Sha256)>,
    inputs: Vec<RouteInput>,
    visible: Vec<VisibleCapture>,
    active_input: Option<InputIndex>,
}

fn identity(session: &Session, core: usize) -> Result<CoreIdentity, String> {
    let value = session.board.cpu(core)?.rprofile_identity(None)?;
    u32::try_from(value).map(CoreIdentity).map_err(|_| "a kernel identity exceeds 32 bits".into())
}

fn task_views(session: &Session, application: &Application) -> Result<Vec<RawView>, String> {
    let mut views = Vec::with_capacity(12);
    for layout in &application.task_layouts {
        for (cell, words) in [(layout.creation, 10), (layout.task, 14), (layout.ids, 1), (layout.domains, 1), (layout.current, 1), (layout.selected, 1)] {
            views.push(RawView::read(session, Guest(cell.0), words)?);
        }
    }
    Ok(views)
}

fn state_views(session: &Session) -> Result<Vec<RawView>, String> {
    let fixed = STATE_VIEWS.iter().map(|&(address, words)| RawView::read(session, Guest(address), words));
    let roots = QUEUE_ROOTS.iter().flat_map(|&root| ROOT_SLOTS.map(|slot| RawView::read(session, Guest(root + slot), 1)));
    fixed.chain(roots).collect()
}

fn first_word(view: &RawView, index: usize) -> u32 { view.words().nth(index).unwrap_or(0) }

/// At the adapter entry its selector block; at a receiving queue entry its envelope; at later receiving queue sites
/// the packet slot, packet header and payload, destination envelope and copied payload as each becomes defined.
fn packet_views(session: &Session, stock: u32, scoped: bool, receiving: bool, registers: &Registers) -> Result<Vec<RawView>, String> {
    let r = |index: usize| registers.r[index].0;
    let mut packet = Vec::new();
    if stock == ADAPTER_ENTRY {
        packet.push(RawView::read(session, Guest(r(0)), 4)?);
    }
    if scoped && stock == QUEUE_ENTRY && r(0) == 1 {
        packet.push(RawView::read(session, Guest(r(1)), 3)?);
    }
    if scoped && stock != QUEUE_ENTRY && QUEUE_SITES.contains(&stock) && receiving {
        let slot = RawView::read(session, Guest(r(11).checked_sub(32).ok_or("the packet slot underflows")?), 1)?;
        let pointer = first_word(&slot, 0);
        packet.push(slot);
        if matches!(stock, RECEIVED | COPY_CALL | RELEASE_CALL) {
            let header = RawView::read(session, Guest(pointer), 3)?;
            let length = first_word(&header, 2);
            if length > PAYLOAD_LIMIT {
                return Err("a queue1 payload exceeds 76 bytes".into());
            }
            packet.push(header);
            if length != 0 {
                let payload = pointer.checked_add(12).ok_or("the packet payload overflows")?;
                packet.push(RawView::read(session, Guest(payload), length.div_ceil(4) as usize)?);
            }
        }
        let destination = RawView::read(session, Guest(r(5)), 3)?;
        let (length, payload) = (first_word(&destination, 1), first_word(&destination, 2));
        packet.push(destination);
        if matches!(stock, COPY_RETURN | RELEASE_CALL | RELEASE_RETURN) && r(4) == 0 && length != 0 {
            if length > PAYLOAD_LIMIT {
                return Err("a received payload exceeds 76 bytes".into());
            }
            packet.push(RawView::read(session, Guest(payload), length.div_ceil(4) as usize)?);
        }
    }
    Ok(packet)
}

fn word_address(address: Address) -> Result<Guest, String> {
    u32::try_from(address.0).map(Guest).map_err(|_| "a render address exceeds a word".into())
}

/// The table records, rows and readable creation records `RenderFacts::probe` reads for task statuses.
fn task_table_views(session: &Session, facts: &crate::render::RenderFacts) -> Result<TaskTableViews, String> {
    let mut views = TaskTableViews {
        entries: facts.task_entries.iter().map(|entry| u32::try_from(entry.0).map(Pc).map_err(|_| "a task entry exceeds a word".to_owned()))
            .collect::<Result<_, _>>()?,
        records: Vec::new(),
        rows: Vec::new(),
        descriptors: Vec::new(),
    };
    for (_, record) in &facts.task_tables {
        let view = RawView::read(session, word_address(*record)?, 2)?;
        let (table, count) = (first_word(&view, 0), first_word(&view, 1));
        views.records.push(view);
        if !(1..=TASK_ROW_LIMIT).contains(&count) {
            continue;
        }
        let words = count as usize * TASK_ROW_WORDS;
        let first = views.rows.len();
        for offset in (0..words).step_by(VIEW_WORDS) {
            views.rows.push(RawView::read(session, Guest(table.wrapping_add(4 * offset as u32)), (words - offset).min(VIEW_WORDS))?);
        }
        for index in 0..count {
            let Some(descriptor) = view_word(&views.rows[first..], table.wrapping_add(index * TASK_ROW_BYTES + 8)) else { continue };
            if let Ok(view) = RawView::read(session, Guest(descriptor), DESCRIPTOR_WORDS) {
                views.descriptors.push(view);
            }
        }
    }
    Ok(views)
}

fn code_digests(session: &Session, application: &Application) -> Result<Vec<(CodeLabel, Sha256)>, String> {
    application.code.iter().map(|range| {
        let mut bytes = vec![0; (range.end - range.start) as usize];
        session.board.read_physical(range.start, &mut bytes)?;
        Ok((range.label, Sha256::of(&bytes)))
    }).collect()
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum WatchAccess {
    CpuFetch,
    CpuRead,
    CpuWrite,
    HostRead,
    HostWrite,
    ModelBusRead,
    ModelBusWrite,
}

#[derive(Deserialize)]
struct WatchClass {
    range: [u64; 2],
    kinds: Vec<WatchAccess>,
}

impl<'a> Recorder<'a> {
    pub fn new(application: &'a Application, role: Role, output: &'a Path) -> Result<Self, String> {
        let mut armed = BTreeMap::new();
        for (index, site) in application.sites.iter().enumerate() {
            let pc = physical(site.pc(role).0);
            armed.insert(pc, Armed::Site(index));
            armed.insert(pc | ALIAS, Armed::Site(index));
        }
        Ok(Self {
            application, output, armed, writer: Some(EvidenceWriter::create(output, EVIDENCE)?), samples: 0, readiness: 0, task1: None,
            scope_sites: Vec::new(), skips: 0, targets: Vec::new(), receives: Vec::new(), search: Search::new(application.readiness),
            publication: None, publication_code: Vec::new(), inputs: Vec::new(), visible: Vec::new(), active_input: None,
        })
    }

    fn label(&self, armed: Armed, pc: u32) -> String {
        match armed {
            Armed::Site(index) => self.application.sites[index].label.as_str().to_owned(),
            Armed::Queue(index) => self.application.queue_sites[index].label.as_str().to_owned(),
            Armed::Target => format!("target-{:x}", physical(pc)),
        }
    }

    /// Both spellings of every application site in the role, labelled; each is a stop.
    pub fn trace_sites(&self) -> BTreeMap<u32, String> {
        self.armed.iter().map(|(&pc, &armed)| (pc, self.label(armed, pc))).collect()
    }

    pub fn owns(&self, hit: &Hit) -> bool { hex_word(&hit.address).is_ok_and(|pc| self.armed.contains_key(&pc)) }

    /// The instruction count the next run must stop at to take a settle-end sample.
    pub fn settle_end(&self) -> Option<u64> { self.search.settle_end() }

    fn writer(&mut self) -> Result<&mut EvidenceWriter, String> { self.writer.as_mut().ok_or_else(|| "the evidence writer is closed".into()) }

    fn receiving(&self, core: Core, registers: &Registers) -> bool {
        self.receives.iter().any(|receive| receive.core == core && registers.r[11].0.checked_add(4) == Some(receive.sp) && registers.r[8].0 == 1)
    }

    /// Samples one live stop, then begins the scope, arms dynamic targets and records publication as the stop demands.
    pub fn hit(&mut self, machine: &mut Machine, hit: &Hit, rows: WatchRows) -> Result<(), String> {
        let pc = hex_word(&hit.address)?;
        let armed = *self.armed.get(&pc).ok_or("a hit at an unarmed application PC")?;
        if hit.instructions != machine.instructions() {
            return Err(format!("the application stop at {pc:#x} is no longer live"));
        }
        let core = Core::from_index(hit.core)?;
        let registers = Registers::of(hit)?;
        let (site, stock, kind) = match armed {
            Armed::Site(index) => {
                let site = &self.application.sites[index];
                (SiteRef::Application(site.label.clone()), site.retained_pc.0, Kind::Site(site.kind))
            }
            Armed::Queue(index) => {
                let site = &self.application.queue_sites[index];
                (SiteRef::Queue(site.label.clone()), site.pc.0, Kind::Queue)
            }
            Armed::Target => (SiteRef::Target, physical(pc), Kind::Target),
        };
        let scoped = matches!(kind, Kind::Queue | Kind::Target);
        let attributed = scoped || matches!(kind, Kind::Site(kind) if kind.task1());
        let receiving = if stock == QUEUE_ENTRY { registers.r[0].0 == 1 } else { self.receiving(core, &registers) };
        let session = &machine.session;
        let code = RawView::read(session, Guest(pc), 1)?;
        let task = if attributed { task_views(session, self.application)? } else { Vec::new() };
        let stack = if attributed && registers.sp.0 % 4 == 0 { vec![RawView::read(session, Guest(registers.sp.0), STACK_WORDS)?] } else { Vec::new() };
        let state = if scoped || stock == CHAIN[0] || stock == TASK1_RESET { state_views(session)? } else { Vec::new() };
        let packet = packet_views(session, stock, scoped, receiving, &registers)?;
        let counts = match session.board.instruction_counts()[..] {
            [first, second] => [Instructions(first), Instructions(second)],
            _ => return Err("expected two core clocks".into()),
        };
        let identities = [identity(session, 0)?, identity(session, 1)?];
        let stop = machine.instructions();
        let index = SampleIndex(self.samples);
        let sample = Sample {
            index, site, pc: Pc(pc), core, stop: Instructions(stop), resume: Instructions(stop + 1), counts, identities, registers, code, task,
            stack, state, packet, input: self.active_input,
        };
        self.search.sample(stock, &sample);
        self.writer()?.sample(&sample)?;
        self.samples += 1;
        if stock == IMAGE5_PUBLICATION && self.publication.is_none() {
            self.publication = Some(Publication { sample: index, row: rows()? });
            self.publication_code = code_digests(&machine.session, self.application)?;
        }
        if stock == CHAIN[0] && self.task1.is_none()
            && let Ok(frame) = committed(self.application, stock, &sample, true)
        {
            self.begin_scope(machine, &sample, frame.domain, [frame.base, frame.end])?;
        }
        let r = |index: usize| registers.r[index].0;
        if scoped && stock == QUEUE_ENTRY && r(0) == 1 {
            self.receives.push(Receive { core, sp: registers.sp.0 });
        }
        // memcpy is not a target: interrupt handlers call it on their own stacks while task1 is current.
        if scoped && receiving && matches!(stock, KERNEL_RECEIVE | RELEASE_CALL) {
            self.add_target(machine, r(3), index)?;
        }
        if scoped && receiving && stock == QUEUE_EXIT {
            self.receives.retain(|receive| receive.core != core || r(11).checked_add(4) != Some(receive.sp));
        }
        Ok(())
    }

    fn begin_scope(&mut self, machine: &mut Machine, entry: &Sample, domain: Domain, stack: [u32; 2]) -> Result<(), String> {
        let application = self.application;
        let current = Domain::ALL.map(|domain| Guest(physical(application.layout(domain).current.0)));
        let config = TraceStackScopeConfig::new(stack.map(Guest), Guest(physical(application.layout(domain).task.0)), current)?;
        machine.traces.begin_stack_scope(config)?;
        self.task1 = Some(Task1 { core: entry.core, entry: entry.index, domain });
        let sites: Vec<(u32, Armed)> = application.queue_sites.iter().enumerate()
            .flat_map(|(index, site)| [physical(site.pc.0), physical(site.pc.0) | ALIAS].map(|pc| (pc, Armed::Queue(index))))
            .collect();
        self.add_scope_sites(machine, &sites)
    }

    fn add_scope_sites(&mut self, machine: &mut Machine, sites: &[(u32, Armed)]) -> Result<(), String> {
        let guests: Vec<Guest> = sites.iter().map(|&(pc, _)| Guest(pc)).collect();
        machine.traces.scope_stack_sites(&machine.session, &guests)?;
        let added: BTreeMap<u32, String> = sites.iter().filter(|(pc, _)| !machine.traces.sites.contains_key(pc))
            .map(|&(pc, armed)| (pc, self.label(armed, pc))).collect();
        machine.traces.add_sites(&machine.session, added)?;
        for &(pc, armed) in sites {
            self.armed.insert(pc, armed);
            self.scope_sites.push(Guest(pc));
        }
        Ok(())
    }

    fn add_target(&mut self, machine: &mut Machine, target: u32, caller: SampleIndex) -> Result<(), String> {
        let target = physical(target);
        if self.targets.iter().any(|known| known.pc.0 == target) {
            return Ok(());
        }
        let armed = |pc: u32| self.armed.contains_key(&pc) || machine.traces.sites.contains_key(&pc);
        if target % 4 != 0 || armed(target) || armed(target | ALIAS) {
            return Err(format!("queue service target {target:#x} is not an independent aligned entry"));
        }
        let word = RawView::read(&machine.session, Guest(target), 1)?.word(target).ok_or("the target's word was not read")?;
        self.targets.push(DynamicTarget { pc: Pc(target), word: CodeWord(word), sample: caller });
        self.add_scope_sites(machine, &[(target, Armed::Target), (target | ALIAS, Armed::Target)])
    }

    /// After every sliced run: drains scope skips, refuses a scope violation, and takes a readiness sample when due.
    pub fn after_run(&mut self, machine: &mut Machine) -> Result<(), String> {
        self.skips += machine.traces.drain_stack_scope_skips().count() as u64;
        if let Some(violation) = machine.traces.stack_scope_summary().and_then(|summary| summary.violation.as_ref()) {
            return Err(format!("the stack scope refused: {violation:?}"));
        }
        match self.search.due(machine.instructions()) {
            Due::Nothing => Ok(()),
            Due::Periodic => self.take_readiness(machine, ReadinessKind::Periodic),
            Due::SettleEnd => self.take_readiness(machine, ReadinessKind::SettleEnd),
        }
    }

    fn take_readiness(&mut self, machine: &Machine, kind: ReadinessKind) -> Result<(), String> {
        let receive = self.search.joined().ok_or("no joined receive is open")?;
        let firmware = &machine.document.firmware;
        let facts = &firmware.render;
        let probe = facts.probe(machine)?;
        let tasks = task_table_views(&machine.session, facts)?;
        let session = &machine.session;
        let idle = [facts.ui_pending, facts.display_suppression, facts.batch_busy, facts.batch_callback, facts.commands_dirty, facts.display_pending]
            .into_iter().map(|address| RawView::read(session, word_address(address)?, 1)).collect::<Result<Vec<_>, String>>()?;
        let keys = firmware.keys.iter().map(|key| {
            let address = firmware.key_table.base.0 + key.row * firmware.key_table.row_bytes + firmware.key_table.released_offset;
            ByteView::read(session, word_address(Address(address))?)
        }).collect::<Result<Vec<_>, String>>()?;
        let render = RenderViews {
            initialized: ByteView::read(session, word_address(facts.initialized)?)?,
            ui_busy: ByteView::read(session, word_address(facts.ui_busy)?)?,
            idle,
            keys,
        };
        let graphics = GraphicsState::of(&session.board.device_save(session.node(&facts.graphics.0)?)?)?;
        let sample = ReadinessSample {
            index: ReadinessIndex(self.readiness), preceding: self.samples, instructions: Instructions(machine.instructions()), receive, kind, probe,
            tasks, render, graphics,
        };
        self.search.readiness(&sample).map_err(|fault| format!("readiness search: {fault:?}"))?;
        self.writer()?.readiness(&sample)?;
        self.readiness += 1;
        Ok(())
    }

    /// Sliced at `PERIOD`, sampling every stop.
    fn advance(&mut self, machine: &mut Machine, count: u64, rows: WatchRows) -> Result<(), String> {
        let end = machine.instructions().checked_add(count).ok_or("the instruction endpoint overflows")?;
        while machine.instructions() < end {
            let before = machine.instructions();
            let outcome = machine.run((end - before).min(PERIOD));
            machine.require_running()?;
            if !matches!(outcome, RunOutcome::Completed | RunOutcome::Stopped) {
                return Err("an application run did not settle".into());
            }
            let hits: Vec<Hit> = machine.traces.hits.drain(..).collect();
            for hit in &hits {
                if !self.owns(hit) {
                    return Err(format!("a hit at {} after the boundary sites were removed", hit.address));
                }
                self.hit(machine, hit, rows)?;
            }
            self.after_run(machine)?;
            if machine.instructions() == before && hits.is_empty() {
                return Err("the observer made no progress".into());
            }
        }
        Ok(())
    }

    fn pulse(&mut self, machine: &mut Machine, key: RouteKey, purpose: InputPurpose, before: Option<Probe>, rows: WatchRows) -> Result<InputIndex, String> {
        let index = InputIndex(self.inputs.len() as u64);
        self.active_input = Some(index);
        let pressed = machine.instructions();
        machine.press(key)?;
        self.advance(machine, PRESS, rows)?;
        machine.release_key(key)?;
        let released = machine.instructions();
        while !machine.document.firmware.released(&machine.session, key)? {
            if machine.instructions() - released >= PRESS {
                return Err("an input release did not debounce".into());
            }
            self.advance(machine, PERIOD, rows)?;
        }
        self.inputs.push(RouteInput {
            key, purpose, pressed: Instructions(pressed), released: Instructions(released), debounced: Instructions(machine.instructions()),
            settled: None, before, after: None,
        });
        Ok(index)
    }

    fn drain(&mut self, machine: &mut Machine, rows: WatchRows) -> Result<Probe, String> {
        let start = machine.instructions();
        loop {
            let probe = machine.document.firmware.render.probe(machine)?;
            if probe.drained && probe.consumer_finished() {
                return Ok(probe);
            }
            if machine.instructions() - start >= SETTLE_LIMIT {
                return Err("the render drain deadline passed".into());
            }
            self.advance(machine, PERIOD, rows)?;
        }
    }

    fn capture(&mut self, machine: &Machine, input: Option<InputIndex>, probe: Probe) -> Result<(), String> {
        let output = self.output;
        let name = format!("application-visible-{:03}", self.visible.len());
        machine.capture_frame()?.write(&output.join(format!("{name}.png")))?;
        let session = &machine.session;
        let save = session.board.device_save(session.node(&machine.document.firmware.render.graphics.0)?)?;
        crate::bridge::write(&output.join(format!("{name}.graphics.json")), &serde_json::to_vec(&save).map_err(|error| error.to_string())?)?;
        let controller = RawView::read(session, Guest(CONTROLLER), CONTROLLER_WORDS)?;
        let menu = machine.menu()?.map(|menu| Menu { menu: menu.menu_id, setting: menu.setting_id, cursor: menu.cursor });
        let bind = |suffix: &str| SideFile::bind(output, &format!("{name}{suffix}"));
        self.visible.push(VisibleCapture {
            input, instructions: Instructions(machine.instructions()), probe, menu, controller, png: bind(".png")?, planes: bind(".planes.bin")?,
            scanout: bind(".scanout.json")?, graphics: bind(".graphics.json")?,
        });
        Ok(())
    }

    fn settle(&mut self, machine: &Machine, input: InputIndex, after: Probe) -> Result<(), String> {
        let slot = &mut self.inputs[input.0 as usize];
        slot.settled = Some(Instructions(machine.instructions()));
        slot.after = Some(after);
        self.capture(machine, Some(input), after)?;
        self.active_input = None;
        Ok(())
    }

    /// Drains, presses, and waits for the press's packet to be consumed with new graphics work, a drained UI and a new
    /// frame; an unhandled probe waits only for the drain.
    fn settled_input(&mut self, machine: &mut Machine, key: RouteKey, purpose: InputPurpose, rows: WatchRows) -> Result<(), String> {
        let before = self.drain(machine, rows)?;
        let old_frame = machine.frame()?;
        let input = self.pulse(machine, key, purpose, Some(before), rows)?;
        if purpose == InputPurpose::UnhandledProbe {
            let after = self.drain(machine, rows)?;
            return self.settle(machine, input, after);
        }
        let start = machine.instructions();
        let mut requested = None;
        loop {
            let after = machine.document.firmware.render.probe(machine)?;
            if after.sender_token != before.sender_token {
                requested = Some(after.sender_token);
            }
            let consumed = requested.is_some_and(|token| {
                after.copied_command & 0xf000 == 0x4000 && after.packet_token == token && after.consumer_token == token
                    && after.copied_command == after.sender_command
                    || after.auxiliary_acknowledged() && after.staged_render_token == token && before.staged_render_token != token
            });
            let new_work = after.graphics.started > before.graphics.started && after.graphics.completed > before.graphics.completed
                && after.graphics.bytes_completed > before.graphics.bytes_completed;
            if consumed && new_work && after.drained && after.consumer_finished() && machine.frame()? != old_frame {
                return self.settle(machine, input, after);
            }
            if machine.instructions() - start >= SETTLE_LIMIT {
                return Err("an input has no acknowledged full-plane render completion".into());
            }
            self.advance(machine, PERIOD, rows)?;
        }
    }

    /// From the boundary: the erased-settings menu, the first-start presses and MENU to the shooting menu, the drained
    /// frame, then the settled inputs.
    pub fn route(&mut self, machine: &mut Machine, menu: &mut MenuObservation, rows: WatchRows) -> Result<(), String> {
        let at = |machine: &Machine, wanted: (u32, u32, u32)| -> Result<bool, String> {
            Ok(machine.menu()?.is_some_and(|menu| (menu.menu_id, menu.setting_id, menu.cursor) == wanted))
        };
        menu.erased_settings = at(&*machine, ERASED_SETTINGS)?;
        if !menu.erased_settings {
            return Err("startup did not reach erased-settings menu".into());
        }
        for key in std::iter::repeat_n(RouteKey::Up, 12).chain([RouteKey::Ok, RouteKey::Ok]) {
            self.pulse(machine, key, InputPurpose::FirstStart, None, rows)?;
        }
        self.advance(machine, 40_000_000, rows)?;
        self.pulse(machine, RouteKey::Ok, InputPurpose::FirstStart, None, rows)?;
        self.advance(machine, 90_000_000, rows)?;
        menu.shooting = at(&*machine, SHOOTING)?;
        if !menu.shooting {
            return Err("route did not reach shooting menu".into());
        }
        self.active_input = None;
        let probe = self.drain(machine, rows)?;
        self.capture(machine, None, probe)?;
        for (key, purpose) in [
            (RouteKey::Down, InputPurpose::Ordinary), (RouteKey::Up, InputPurpose::Ordinary), (RouteKey::Back, InputPurpose::BackHandback),
            (RouteKey::Ok, InputPurpose::AfterBack), (RouteKey::Left, InputPurpose::UnhandledProbe),
        ] {
            self.settled_input(machine, key, purpose, rows)?;
        }
        Ok(())
    }

    /// Records the code writes and write classes from the watch, the code digests and scope, and closes the evidence.
    pub fn finish(mut self, machine: &mut Machine, watch_state: &serde_json::Value) -> Result<ApplicationReceipt, String> {
        self.skips += machine.traces.drain_stack_scope_skips().count() as u64;
        let rows: Vec<WatchRow> = serde_json::from_value(watch_state["records"].clone()).map_err(|error| format!("watch rows: {error}"))?;
        let mut writes = 0u64;
        for (row, write) in rows.iter().enumerate() {
            let end = write.physical.saturating_add(write.size.max(1));
            if self.application.code.iter().any(|range| range.overlaps(write.physical, end)) {
                let write = ApplicationWrite { row: row as u64, source: write.source, physical: write.physical, size: write.size, core: write.core, pc: write.pc };
                self.writer()?.write(&write)?;
                writes += 1;
            }
        }
        let classes: Vec<WatchClass> = serde_json::from_value(watch_state["classes"].clone()).map_err(|error| format!("watch classes: {error}"))?;
        let watch_classes = classes.into_iter().filter(|class| class.kinds.contains(&WatchAccess::CpuWrite))
            .map(|class| ByteRange { start: class.range[0], end: class.range[1] }).collect();
        let end = code_digests(&machine.session, self.application)?;
        let code = end.into_iter().map(|(label, end)| CodeObservation {
            label,
            publication: self.publication_code.iter().find(|(observed, _)| *observed == label).map(|(_, digest)| digest.clone()),
            end,
        }).collect();
        let scope = machine.traces.stack_scope_summary().cloned()
            .map(|summary| ScopeObservation { summary, sites: std::mem::take(&mut self.scope_sites), skips: self.skips });
        let evidence = self.writer.take().ok_or("the evidence writer is closed")?.finish()?;
        Ok(ApplicationReceipt {
            evidence, samples: self.samples, readiness: self.readiness, writes, task1: self.task1, publication: self.publication, code, scope,
            targets: self.targets, first_ready: self.search.first_ready(), watch_classes, inputs: self.inputs, visible: self.visible,
        })
    }
}
