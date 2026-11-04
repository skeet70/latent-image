use fujimod_xm1::application_observation::{
    check_evidence, check_pair, read_evidence, write_evidence, ApplicationCheck, ApplicationReceipt, ApplicationRefusal, ApplicationWrite,
    ByteView, CodeObservation, Control, CoreIdentity, DynamicTarget, Evidence, Expected, FirstReady, GraphicsState, InputIndex, InputPurpose,
    Menu, PairField, Publication, RawView, ReadinessIndex, ReadinessKind, ReadinessSample, Registers, RenderViews, RoleEvidence, RouteInput,
    Sample, SampleIndex, ScopeObservation, SideFile, SiteRef, Task1, TaskTableViews, VisibleCapture, EVIDENCE,
};
use fujimod_xm1::composed_catalog::{
    Application, ApplicationProblem, CatalogRefusal, CodeLabel, CodeWord, Contract, Core, Domain, Instructions, Pc, QueueLabel, Role, Schema,
    Sha256, SiteLabel, APPLICATION_SITES, QUEUE_SITES,
};
use fujimod_xm1::composed_receipt::{ByteRange, HexBytes, Identity, MenuObservation, Outcome, Receipt, ReceiptKind, Word, WriteKind};
use fujimod_xm1::guest::RouteKey;
use fujimod_xm1::provenance::Guest;
use fujimod_xm1::render::{Graphics, Probe};
use fujimod_xm1::run::CoreIdentities;
use fujimod_xm1::trace::{EnteredInstructions, ScopedHitCount, TraceStackScopeConfig, TraceStackScopeSummary, TraceStackSiteSummary};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

const FIXTURE: &str = include_str!("data/composed-contract-v4.json");
const ALIAS: u32 = 0x4000_0000;
const SETTLE: u64 = 1_000_000;
const START: u64 = 80_000_000;
const PUMP: u32 = 0xbfbd20;
const CREATION: u32 = 0x4005beac;
const TASK: u32 = 0x4005ff0c;
const IDS: u32 = 0x400622d4;
const CURRENT: u32 = 0x40054760;
const STACK_BASE: u32 = 0x200000;
const STACK_END: u32 = 0x200c00;
const SELECTOR_BLOCK: u32 = 0x210000;
const PACKET: u32 = 0x220000;
const COPIED: u32 = 0x230000;
const COMMAND: u32 = 0x1234;
const PAYLOAD: [u32; 2] = [0xaabbccdd, 0x11223344];
const KERNEL_TARGET: u32 = 0x400000;
const RELEASE_TARGET: u32 = 0x400100;
const ROOT_A: u32 = 0x4004dae0;
const ROOT_B: u32 = 0x400ddaa0;
const RENDER: u32 = 0x500000;
const TABLE_RECORD: u32 = 0x510000;
const TABLE: u32 = 0x520000;
const DESCRIPTORS: u32 = 0x530000;
const ENTRIES: [u32; 4] = [0xc22f60, 0x111000, 0x112000, 0x113000];
const PERIOD: u64 = 250_000;
const NORMALIZED: u32 = 0x77;

/// The readiness samples one receive takes while blocked.
#[derive(Clone, Copy, PartialEq)]
enum Plan {
    Quiet,
    /// Ready through the settle interval: first-ready.
    Stable,
    /// Ready, then not ready inside the settle interval.
    Unstable,
    /// Ready less than one settle interval before the search end.
    PastEnd,
}

fn physical(address: u32) -> u32 { if (ALIAS..0x6000_0000).contains(&address) { address - ALIAS } else { address } }

fn digest(byte: char) -> Sha256 { Sha256::try_from(byte.to_string().repeat(64)).unwrap() }

fn call(offset: u32, target: u32) -> u32 { 0xeb00_0000 | (((i64::from(target) - i64::from(PUMP + offset) - 8) / 4) as u32 & 0xff_ffff) }

fn pump_words() -> [u32; 8] {
    [0xe92d4010, call(4, 0xbfb1b8), call(8, 0xbfb20c), 0xe1a04000, call(16, 0xbfb1e0), 0xe1a00004, 0xe8bd8010, 0xe320f000]
}

fn word(pc: u32) -> u32 {
    if (PUMP..PUMP + 32).contains(&pc) { pump_words()[((pc - PUMP) / 4) as usize] } else { 0xe1a0_0000 | (pc & 0xfff) }
}

fn layout(domain: u8, cells: [u32; 6]) -> Value {
    json!({"domain": domain, "creation": cells[0], "task": cells[1], "ids": cells[2], "domains": cells[3], "current": cells[4], "selected": cells[5]})
}

fn schema5() -> Value {
    let mut value: Value = serde_json::from_str(FIXTURE).unwrap();
    value["schema"] = json!(5);
    let code = |label: &str, start: u32, end: u32| json!({"label": label, "start": start, "end": end, "retained_sha256": digest('a'), "composed_sha256": digest('b')});
    value["application"] = json!({
        "sites": APPLICATION_SITES.iter().map(|&(pc, kind)| json!({
            "label": format!("app-{pc:x}"), "kind": kind, "retained_pc": pc, "composed_pc": pc, "word": word(pc),
        })).collect::<Vec<_>>(),
        "queue_sites": QUEUE_SITES.iter().map(|&pc| json!({"label": format!("queue-{pc:x}"), "pc": pc, "word": word(pc)})).collect::<Vec<_>>(),
        "task_layouts": [
            layout(0, [CREATION, TASK, IDS, 0x400545cc, CURRENT, 0x40054768]),
            layout(1, [0x400ebe6c, 0x400efecc, 0x400f2294, 0x400e458c, 0x400e4720, 0x400e4728]),
        ],
        "code": [code("task1-body", 0xc22f60, 0xc2327c), code("task1-setup", 0xbfb124, 0xbfb1b8), code("pump", PUMP, PUMP + 32)],
        "readiness": {"settle": SETTLE, "end": START},
    });
    value
}

fn parse(value: &Value) -> Result<Contract, CatalogRefusal> { Contract::parse(&serde_json::to_vec(value).unwrap()) }

fn refused(edit: impl FnOnce(&mut Value)) -> CatalogRefusal {
    let mut value = schema5();
    edit(&mut value);
    parse(&value).unwrap_err()
}

fn scratch() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!("application-observation-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn a_schema_five_contract_parses_its_application_section() {
    let contract = parse(&schema5()).unwrap();
    assert_eq!(contract.schema(), Schema::Five);
    let application = contract.application().unwrap();
    assert_eq!(application.sites.len(), APPLICATION_SITES.len());
    assert_eq!(application.pump.entry, Pc(PUMP));
    assert_eq!(application.pump.calls.map(|call| call.target), [Pc(0xbfb1b8), Pc(0xbfb20c), Pc(0xbfb1e0)]);
    assert_eq!(application.pump.returned, Pc(PUMP + 24));
    assert!(serde_json::to_value(contract.plan()).unwrap().get("application").is_some());
}

#[test]
fn a_schema_four_contract_has_no_application_and_an_unchanged_plan() {
    let contract = Contract::parse(FIXTURE.as_bytes()).unwrap();
    assert_eq!(contract.schema(), Schema::Four);
    assert!(contract.application().is_none());
    assert!(serde_json::to_value(contract.plan()).unwrap().get("application").is_none());
}

#[test]
fn the_application_section_must_match_the_schema() {
    assert_eq!(refused(|value| value["schema"] = json!(4)), CatalogRefusal::Application(ApplicationProblem::Section));
    assert_eq!(refused(|value| { value.as_object_mut().unwrap().remove("application"); }), CatalogRefusal::Application(ApplicationProblem::Section));
}

#[test]
fn site_labels_sets_and_placement_refuse() {
    assert_eq!(refused(|value| value["application"]["sites"][0]["label"] = json!("app-1")),
        CatalogRefusal::Application(ApplicationProblem::SiteLabel(Pc(APPLICATION_SITES[0].0))));
    assert_eq!(refused(|value| { value["application"]["sites"].as_array_mut().unwrap().pop(); }), CatalogRefusal::Application(ApplicationProblem::Sites));
    assert_eq!(refused(|value| value["application"]["sites"][0]["kind"] = json!("uilib")), CatalogRefusal::Application(ApplicationProblem::Sites));
    assert_eq!(refused(|value| { value["application"]["queue_sites"].as_array_mut().unwrap().pop(); }),
        CatalogRefusal::Application(ApplicationProblem::QueueSites));
    let owned = |value: &mut Value| value["pieces"][1]["catalog"]["owned_code"] = json!([{"start": 263216, "end": 263924}, {"start": 0xc22f00u32, "end": 0xc23000u32}]);
    assert_eq!(refused(owned), CatalogRefusal::Application(ApplicationProblem::SiteInPiece(Pc(0xc22f60))));
    let collide = |value: &mut Value| value["pieces"][0]["catalog"]["boundaries"][1]["retained_pc"] = json!(0x725214u32);
    assert_eq!(refused(collide), CatalogRefusal::Application(ApplicationProblem::SiteCollision(Pc(0x725214))));
}

#[test]
fn a_pump_that_does_not_call_the_hook_targets_refuses() {
    let index = APPLICATION_SITES.iter().position(|&(pc, _)| pc == PUMP + 4).unwrap();
    assert_eq!(refused(|value| value["application"]["sites"][index]["word"] = json!(0xe1a00000u32)),
        CatalogRefusal::Application(ApplicationProblem::Pump));
}

#[test]
fn layouts_code_ranges_and_readiness_refuse_when_malformed() {
    assert_eq!(refused(|value| value["application"]["task_layouts"][0]["domain"] = json!(1)), CatalogRefusal::Application(ApplicationProblem::TaskLayouts));
    assert_eq!(refused(|value| value["application"]["code"][1]["label"] = json!("pump")), CatalogRefusal::Application(ApplicationProblem::CodeRanges));
    assert_eq!(refused(|value| value["application"]["readiness"]["settle"] = json!(0)), CatalogRefusal::Application(ApplicationProblem::Readiness));
    assert!(matches!(refused(|value| value["application"]["extra"] = json!(1)), CatalogRefusal::Shape(_)));
}

#[derive(Clone, Default)]
struct Memory(BTreeMap<u32, u32>);

impl Memory {
    fn set(&mut self, address: u32, word: u32) { self.0.insert(physical(address), word); }
    fn view(&self, address: u32, words: u32) -> RawView {
        let bytes: Vec<u8> = (0..words).flat_map(|index| self.0.get(&(physical(address) + 4 * index)).copied().unwrap_or(0).to_le_bytes()).collect();
        let bytes = HexBytes(bytes);
        RawView { address: Guest(address), physical: bytes.clone(), spellings: [[bytes.clone(), bytes.clone()], [bytes.clone(), bytes]] }
    }
}

fn registers(r: [u32; 13], sp: u32, lr: u32) -> Registers { Registers { r: r.map(Word), sp: Word(sp), lr: Word(lr), cpsr: Word(0x13) } }

fn with(base: [u32; 13], changes: &[(usize, u32)]) -> [u32; 13] {
    let mut r = base;
    for &(index, value) in changes {
        r[index] = value;
    }
    r
}

fn probe(counter: u64) -> Probe {
    Probe {
        copied_command: 0x4001, packet_token: 1, sender_token: 1, consumer_token: 1, sender_command: 0x4001, staged_render_command: 0,
        staged_render_token: 0, drained: true, uilib_waiting: true,
        graphics: Graphics { idle: true, started: counter, completed: counter, bytes_completed: counter * 10 },
    }
}

fn graphics(counter: u64) -> GraphicsState {
    GraphicsState { busy: false, command_pending: false, control: 0, jobs_started: counter, jobs_completed: counter, bytes_completed: counter * 10, acknowledgments: counter }
}

struct Builder<'a> {
    application: &'a Application,
    memory: Memory,
    samples: Vec<Sample>,
    readiness: Vec<ReadinessSample>,
    clock: u64,
    input: Option<InputIndex>,
    first_ready: Option<FirstReady>,
    targets: Vec<DynamicTarget>,
}

const BASE: [u32; 13] = [0, 0, 0, 0, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc];

impl<'a> Builder<'a> {
    fn new(application: &'a Application) -> Self {
        let mut memory = Memory::default();
        for site in &application.sites {
            memory.set(site.retained_pc.0, site.word.0);
        }
        for site in &application.queue_sites {
            memory.set(site.pc.0, site.word.0);
        }
        for (address, value) in [
            (CREATION + 8, 0xc22f60), (CREATION + 12, 5), (CREATION + 16, 0xc00), (CREATION + 20, STACK_BASE), (CREATION + 32, 0x10001),
            (CREATION + 36, 1), (IDS, 0x10001), (TASK + 8, CREATION), (TASK + 32, 1), (CURRENT, TASK),
            (0x96bd20, ROOT_A), (0x96bd24, ROOT_B), (ROOT_A + 0xc8, KERNEL_TARGET), (ROOT_A + 0xf8, RELEASE_TARGET),
            (SELECTOR_BLOCK, 0x107), (PACKET + 4, COMMAND), (PACKET + 8, 8), (PACKET + 12, PAYLOAD[0]), (PACKET + 16, PAYLOAD[1]),
            (COPIED, PAYLOAD[0]), (COPIED + 4, PAYLOAD[1]), (KERNEL_TARGET, 0xe92d4800), (RELEASE_TARGET, 0xe92d4810),
            (RENDER, 1), (TABLE_RECORD, TABLE), (TABLE_RECORD + 4, ENTRIES.len() as u32),
        ] {
            memory.set(address, value);
        }
        for (index, entry) in ENTRIES.into_iter().enumerate() {
            let (row, descriptor) = (TABLE + 0x38 * index as u32, DESCRIPTORS + 0x40 * index as u32);
            for (address, value) in [(row + 8, descriptor), (row + 12, 0x12), (descriptor + 8, entry), (descriptor + 36, 1)] {
                memory.set(address, value);
            }
        }
        Self { application, memory, samples: Vec::new(), readiness: Vec::new(), clock: 1000, input: None, first_ready: None, targets: Vec::new() }
    }

    fn task_views(&self) -> Vec<RawView> {
        self.application.task_layouts.iter().flat_map(|layout| {
            [(layout.creation, 10), (layout.task, 14), (layout.ids, 1), (layout.domains, 1), (layout.current, 1), (layout.selected, 1)]
                .map(|(cell, words)| self.memory.view(cell.0, words))
        }).collect()
    }

    fn state_views(&self) -> Vec<RawView> {
        let mut views = vec![self.memory.view(0x944580, 20), self.memory.view(0x1467588, 80), self.memory.view(0x14676c8, 7),
            self.memory.view(0x96bd20, 2), self.memory.view(0x7cad9c, 1)];
        for root in [ROOT_A, ROOT_B] {
            for slot in [0xc8, 0xf8, 0x1618] {
                views.push(self.memory.view(root + slot, 1));
            }
        }
        views
    }

    fn push(&mut self, site: SiteRef, pc: u32, core: Core, registers: Registers, packet: Vec<RawView>) -> SampleIndex {
        let index = SampleIndex(self.samples.len() as u64);
        let stop = self.clock;
        self.clock += 100;
        let (code, task, state) = (self.memory.view(pc, 1), self.task_views(), self.state_views());
        self.samples.push(Sample {
            index, site, pc: Pc(pc), core, stop: Instructions(stop), resume: Instructions(stop + 1),
            counts: [Instructions(stop), Instructions(0)], identities: CoreIdentities::Xm1.words().map(CoreIdentity), registers,
            code, task, stack: Vec::new(), state, packet, input: self.input,
        });
        index
    }

    fn site(&mut self, pc: u32, registers: Registers) -> SampleIndex { self.site_on(Core::Primary, pc, registers, Vec::new()) }

    fn site_on(&mut self, core: Core, pc: u32, registers: Registers, packet: Vec<RawView>) -> SampleIndex {
        self.push(SiteRef::Application(SiteLabel::of(Pc(pc))), pc, core, registers, packet)
    }

    fn queue(&mut self, pc: u32, registers: Registers, packet: Vec<RawView>) -> SampleIndex {
        self.push(SiteRef::Queue(QueueLabel::of(Pc(pc))), pc, Core::Primary, registers, packet)
    }

    fn target(&mut self, pc: u32, registers: Registers, caller: SampleIndex) -> SampleIndex {
        if !self.targets.iter().any(|target| target.pc.0 == pc) {
            self.targets.push(DynamicTarget { pc: Pc(pc), word: CodeWord(self.memory.0[&pc]), sample: caller });
        }
        self.push(SiteRef::Target, pc, Core::Primary, registers, Vec::new())
    }

    fn startup(&mut self) -> SampleIndex {
        let secondary = |r: [u32; 13], sp| registers(r, sp, 0);
        self.site_on(Core::Secondary, 0x6a9840, secondary(BASE, 0x300000), Vec::new());
        self.site_on(Core::Secondary, 0x6a9934, secondary(BASE, 0x300000), Vec::new());
        self.site_on(Core::Secondary, 0x69d2e0, secondary(with(BASE, &[(3, 0xd985a4)]), 0x300000), Vec::new());
        self.site_on(Core::Secondary, 0xd985a4, secondary(BASE, 0x2ffff8), Vec::new());
        self.site_on(Core::Secondary, 0xd985c8, secondary(BASE, 0x2ffff0), Vec::new());
        self.site_on(Core::Secondary, 0xd985d8, secondary(BASE, 0x2ffff0), Vec::new());
        self.site_on(Core::Secondary, 0x69d2e4, secondary(BASE, 0x300000), Vec::new());
        let entry = self.site(0xc22f60, registers(with(BASE, &[(1, TASK), (2, CREATION)]), STACK_END, 0x453f0));
        for pc in [0xbfb124, 0xc22f70, 0xc22bd0, 0xc22f74, 0xc22f84] {
            self.site(pc, registers(BASE, STACK_END - 0x40, 0));
        }
        entry
    }

    fn ready(&mut self, preceding: u64, receive: SampleIndex, kind: ReadinessKind, ready: bool) {
        let byte = |memory: &Memory, address: u32| ByteView { address: Guest(address), view: memory.view(address & !3, 1) };
        let mut render = RenderViews {
            initialized: byte(&self.memory, RENDER), ui_busy: byte(&self.memory, RENDER + 4),
            idle: (0..6).map(|index| self.memory.view(RENDER + 0x10 + 4 * index, 1)).collect(), keys: vec![byte(&self.memory, RENDER + 0x40)],
        };
        if !ready {
            let busy = &mut render.ui_busy.view;
            for bytes in std::iter::once(&mut busy.physical).chain(busy.spellings.iter_mut().flatten()) {
                bytes.0[0] = 1;
            }
        }
        let tasks = TaskTableViews {
            entries: ENTRIES.map(Pc).to_vec(),
            records: vec![self.memory.view(TABLE_RECORD, 2)],
            rows: vec![self.memory.view(TABLE, 14 * ENTRIES.len() as u32)],
            descriptors: (0..ENTRIES.len() as u32).map(|index| self.memory.view(DESCRIPTORS + 0x40 * index, 10)).collect(),
        };
        self.readiness.push(ReadinessSample {
            index: ReadinessIndex(self.readiness.len() as u64), preceding, instructions: Instructions(self.clock), receive, kind,
            probe: Probe { drained: ready, ..probe(1) }, tasks, render, graphics: graphics(1),
        });
    }

    /// Readiness samples every `PERIOD` while blocked.
    fn blocked(&mut self, receive: SampleIndex, plan: Plan) {
        let preceding = self.samples.len() as u64;
        self.clock += PERIOD;
        if plan == Plan::PastEnd {
            self.clock = self.clock.max(START - 2 * PERIOD);
        }
        let start = self.clock;
        match plan {
            Plan::Quiet => {}
            Plan::Stable => {
                let first = ReadinessIndex(self.readiness.len() as u64);
                for _ in 0..SETTLE / PERIOD {
                    self.ready(preceding, receive, ReadinessKind::Periodic, true);
                    self.clock += PERIOD;
                }
                self.ready(preceding, receive, ReadinessKind::SettleEnd, true);
                self.first_ready = Some(FirstReady { readiness: first, receive, instructions: Instructions(start) });
            }
            Plan::Unstable => {
                self.ready(preceding, receive, ReadinessKind::Periodic, true);
                self.clock += PERIOD;
                self.ready(preceding, receive, ReadinessKind::Periodic, false);
            }
            Plan::PastEnd => {
                self.ready(preceding, receive, ReadinessKind::Periodic, true);
                self.clock += PERIOD;
                self.ready(preceding, receive, ReadinessKind::Periodic, true);
            }
        }
        self.clock += 100;
    }

    /// One receive through the adapter and queue1 with its kernel, copy and release calls, then one pump.
    fn cycle(&mut self, plan: Plan) {
        let call_sp = STACK_END - 0x80;
        let queue_sp = call_sp - 32;
        let destination = call_sp - 28;
        let frame = queue_sp - 4;
        let inner = queue_sp - 0x40;
        self.memory.set(frame - 32, PACKET);
        self.memory.set(destination, COMMAND);
        self.memory.set(destination + 4, 8);
        self.memory.set(destination + 8, COPIED);
        let task1 = registers(with(BASE, &[(0, SELECTOR_BLOCK)]), call_sp, 0);
        let receive = self.site(0xc2314c, task1);
        let selector = vec![self.memory.view(SELECTOR_BLOCK, 4)];
        self.site_on(Core::Primary, 0xdb5124, registers(task1.r.map(|word| word.0), call_sp, 0xc23150), selector);
        let blocking = registers(with(BASE, &[(0, 1), (1, call_sp - 28), (2, u32::MAX)]), queue_sp, 0);
        self.site(0xdb5170, blocking);
        let envelope = vec![self.memory.view(destination, 3)];
        self.queue(0x725318, registers(blocking.r.map(|word| word.0), queue_sp, 0xdb5174), envelope);
        let queued = with(BASE, &[(4, 0), (5, destination), (6, 0), (8, 1), (11, frame)]);
        let slot = self.memory.view(frame - 32, 1);
        let header = vec![slot.clone(), self.memory.view(PACKET, 3), self.memory.view(PACKET + 12, 2), self.memory.view(destination, 3)];
        let released = [header.clone(), vec![self.memory.view(COPIED, 2)]].concat();
        let kernel = registers(with(queued, &[(0, 1), (1, frame - 32), (2, u32::MAX), (3, KERNEL_TARGET)]), inner, 0);
        let kernel_call = self.queue(0x725378, kernel, vec![slot.clone(), self.memory.view(destination, 3)]);
        self.target(KERNEL_TARGET, registers(kernel.r.map(|word| word.0), inner, 0x72537c), kernel_call);
        self.blocked(receive, plan);
        self.queue(0x72537c, registers(with(queued, &[(0, 0)]), inner, 0), vec![slot.clone(), self.memory.view(destination, 3)]);
        self.queue(0x7253c0, registers(queued, inner, 0), header.clone());
        let copy = registers(with(queued, &[(0, COPIED), (1, PACKET + 12), (2, 8)]), inner, 0);
        self.queue(0x7253f8, copy, header.clone());
        self.queue(0x7253fc, registers(queued, inner, 0), released.clone());
        let release = registers(with(queued, &[(0, 1), (1, PACKET), (3, RELEASE_TARGET)]), inner, 0);
        let release_call = self.queue(0x725414, release, released.clone());
        self.target(RELEASE_TARGET, registers(release.r.map(|word| word.0), inner, 0x725418), release_call);
        self.queue(0x725418, registers(queued, inner, 0), released.clone());
        self.queue(0x72541c, registers(with(queued, &[(0, 0)]), inner, 0), released);
        self.site(0xdb5174, registers(with(BASE, &[(0, 0)]), queue_sp, 0));
        self.site(0xdb5178, registers(with(BASE, &[(5, 0)]), queue_sp, 0));
        self.site(0xdb51d0, registers(with(BASE, &[(5, 0)]), queue_sp, 0));
        self.site(0xc23150, registers(BASE, call_sp, 0));
        self.pump();
        self.site(0xc22f84, registers(BASE, STACK_END - 0x40, 0));
    }

    fn pump(&mut self) {
        let caller_sp = STACK_END - 0x80;
        let inner = caller_sp - 8;
        let caller = registers(BASE, caller_sp, 0);
        self.site(0xc23274, caller);
        self.site(PUMP, registers(BASE, caller_sp, 0xc23278));
        let body = |r0: u32, lr: u32| registers(with(BASE, &[(0, r0)]), inner, lr);
        for (site, target) in [(PUMP + 4, 0xbfb1b8), (PUMP + 8, 0xbfb20c), (PUMP + 16, 0xbfb1e0)] {
            let result = if target == 0xbfb20c { NORMALIZED } else { 0 };
            if site != PUMP + 4 {
                self.site(site, body(result, 0));
            } else {
                self.site(site, body(0, 0));
            }
            self.site(target, body(if site != PUMP + 4 { result } else { 0 }, site + 4));
            if site == PUMP + 8 {
                self.site(site + 4, body(NORMALIZED, 0));
            }
        }
        self.site(PUMP + 20, body(NORMALIZED, 0));
        self.site(PUMP + 24, body(NORMALIZED, 0));
        self.site(0xc23278, registers(with(BASE, &[(0, NORMALIZED)]), caller_sp, 0));
    }
}

fn press(index: usize) -> u64 { if index < 15 { START + index as u64 * 20_000_000 } else { START + 300_000_000 + (index as u64 - 15) * 40_000_000 } }

fn route() -> Vec<RouteInput> {
    let keys = std::iter::repeat_n((RouteKey::Up, InputPurpose::FirstStart), 12)
        .chain(std::iter::repeat_n((RouteKey::Ok, InputPurpose::FirstStart), 3))
        .chain([(RouteKey::Down, InputPurpose::Ordinary), (RouteKey::Up, InputPurpose::Ordinary), (RouteKey::Back, InputPurpose::BackHandback),
            (RouteKey::Ok, InputPurpose::AfterBack), (RouteKey::Left, InputPurpose::UnhandledProbe)]);
    keys.enumerate().map(|(index, (key, purpose))| {
        let pressed = press(index);
        let settled = (index >= 15).then(|| Instructions(pressed + 30_000_000));
        RouteInput {
            key, purpose, pressed: Instructions(pressed), released: Instructions(pressed + 8_000_000), debounced: Instructions(pressed + 8_000_000),
            settled, before: settled.map(|_| probe(index as u64 - 13)), after: settled.map(|_| probe(index as u64 - 12)),
        }
    }).collect()
}

fn scanout(png: &SideFile, planes: &SideFile) -> Value {
    json!({
        "schema": 1, "instructions": 0, "node": "scanout", "instance": 0,
        "pixels": {"width": 4, "height": 2, "rgb_bytes": 24, "rgb_sha256": digest('c')},
        "png": {"path": format!("/out/{}", png.path.as_str()), "sha256": png.sha256, "bytes": png.bytes},
        "source_bytes": {"path": format!("/out/{}", planes.path.as_str()), "sha256": planes.sha256, "bytes": planes.bytes},
        "source_planes": [{"kind": "main", "native_source_address": 0x100000, "offset": 0, "bytes": 16}],
        "native_scanout_state": {"instances": [{"instance": 0, "image_base": "0xfff6b000",
            "registers": {"0xfff6b000": "0x00000001", "0xfff6b028": "0x00030001", "0xfff6b02c": "0x00100000"}}], "backdrop_profile": null},
        "native_frame_state": {"instance": 0, "image_base": "0xfff6b000", "width": 4, "height": 2,
            "main": {"width": 4, "height": 2, "source": "0x00100000", "stride": 8}, "source": null, "stride": null, "canvas": null},
    })
}

fn visible(directory: &std::path::Path, inputs: &[RouteInput], memory: &Memory) -> Vec<VisibleCapture> {
    (0..6).map(|index| {
        let name = format!("application-visible-{index:03}");
        let input = (index > 0).then(|| InputIndex(14 + index as u64));
        let probe = input.map_or(probe(2), |id| inputs[id.0 as usize].after.unwrap());
        let png = SideFile::replace(directory, &format!("{name}.png"), format!("png {index}").as_bytes()).unwrap();
        let planes = SideFile::replace(directory, &format!("{name}.planes.bin"), &[index as u8; 16]).unwrap();
        let scanout = SideFile::replace(directory, &format!("{name}.scanout.json"), &serde_json::to_vec(&scanout(&png, &planes)).unwrap()).unwrap();
        let counter = probe.graphics.started;
        let save = json!({"engine": {"regs": [], "busy": false, "command_pending": null, "jobs_started": counter, "jobs_completed": counter,
            "bytes_completed": counter * 10, "acknowledgments": counter}});
        let graphics = SideFile::replace(directory, &format!("{name}.graphics.json"), &serde_json::to_vec(&save).unwrap()).unwrap();
        let instructions = input.map_or(Instructions(press(15) - 1_000_000), |id| inputs[id.0 as usize].settled.unwrap());
        VisibleCapture {
            input, instructions, probe, menu: Some(Menu { menu: 1, setting: 5, cursor: 2 }), controller: memory.view(0x944580, 20), png, planes, scanout, graphics,
        }
    }).collect()
}

struct Scenario {
    contract: Contract,
    receipt: Receipt,
    evidence: Evidence,
    directory: PathBuf,
}

impl Scenario {
    fn new() -> Self { Self::with(&[Plan::Stable, Plan::Quiet]) }

    /// Receives before the route, one per plan.
    fn with(plans: &[Plan]) -> Self {
        let contract = parse(&schema5()).unwrap();
        let directory = scratch();
        let application = contract.application().unwrap();
        let mut builder = Builder::new(application);
        let entry = builder.startup();
        for &plan in plans {
            builder.cycle(plan);
        }
        let inputs = route();
        for index in 15..20 {
            builder.clock = press(index) + 1_000_000;
            builder.input = Some(InputIndex(index as u64));
            builder.cycle(Plan::Quiet);
        }
        builder.input = None;
        let samples = &builder.samples;
        let scoped = |sample: &Sample| matches!(sample.site, SiteRef::Queue(_) | SiteRef::Target);
        let sites: BTreeSet<u32> = QUEUE_SITES.iter().copied().chain(builder.targets.iter().map(|target| target.pc.0))
            .flat_map(|pc| [pc, pc | ALIAS]).collect();
        let summary = TraceStackScopeSummary {
            config: TraceStackScopeConfig::new([Guest(STACK_BASE), Guest(STACK_END)], Guest(physical(TASK)),
                [Guest(physical(CURRENT)), Guest(physical(0x400e4720))]).unwrap(),
            sites: sites.iter().map(|&site| TraceStackSiteSummary {
                site: Guest(site), entered: EnteredInstructions(0),
                recorded: Core::ALL.map(|core| ScopedHitCount(samples.iter().filter(|sample| scoped(sample) && sample.pc.0 == site && sample.core == core).count() as u64)),
                outside: [ScopedHitCount(0); 2],
            }).collect(),
            violation: None,
        };
        let evidence = Evidence {
            samples: builder.samples.clone(), readiness: builder.readiness.clone(),
            writes: vec![ApplicationWrite { row: 3, source: WriteKind::CpuWrite, physical: u64::from(PUMP), size: 4, core: Some(Core::Secondary), pc: Some(0x6a0000) }],
        };
        let visible = visible(&directory, &inputs, &builder.memory);
        let section = ApplicationReceipt {
            evidence: write_evidence(&directory, EVIDENCE, &evidence).unwrap(),
            samples: evidence.samples.len() as u64, readiness: evidence.readiness.len() as u64, writes: evidence.writes.len() as u64,
            task1: Some(Task1 { core: Core::Primary, entry, domain: Domain::A }),
            publication: Some(Publication { sample: SampleIndex(0), row: 10 }),
            code: application.code.iter().map(|range| CodeObservation {
                label: range.label, publication: Some(range.sha256(Role::Composed).clone()), end: range.sha256(Role::Composed).clone(),
            }).collect(),
            scope: Some(ScopeObservation { summary, sites: sites.iter().map(|&site| Guest(site)).collect(), skips: 0 }),
            targets: builder.targets.clone(),
            first_ready: builder.first_ready,
            watch_classes: application.code.iter().map(|range| ByteRange { start: range.start, end: range.end }).collect(),
            inputs,
            visible,
        };
        let receipt = Receipt {
            schema: Schema::Five, kind: ReceiptKind::ComposedStartupReceipt, role: Role::Composed,
            identity: Identity { run: digest('1'), update: contract.identities(Role::Composed).dat_sha256.clone(), boot_rom: digest('2'), observer: digest('3'), contract: digest('4') },
            policies: vec![], outcome: Outcome::Observed, error: None, machine_failure: None, instructions: 0, rows: vec![], counts: vec![],
            off_core: vec![], attach: None, attach_sources: None, guard: None, menu: MenuObservation { erased_settings: true, shooting: true },
            application: Some(section),
        };
        Self { contract, receipt, evidence, directory }
    }

    fn section(&self) -> &ApplicationReceipt { self.receipt.application.as_ref().unwrap() }

    fn check(&self) -> Result<(), ApplicationRefusal> {
        check_evidence(&RoleEvidence {
            application: self.contract.application().unwrap(), role: Role::Composed, receipt: self.section(), evidence: &self.evidence,
            directory: &self.directory,
        })
    }

    fn refusal(&self) -> Option<ApplicationCheck> { self.check().err().map(|refusal| refusal.check) }

    fn control(mut self, control: Control) -> Self {
        let application = self.contract.application().unwrap().clone();
        control.apply(&application, &mut self.receipt, &mut self.evidence, &self.directory).unwrap();
        let section = self.receipt.application.as_mut().unwrap();
        section.evidence = write_evidence(&self.directory, EVIDENCE, &self.evidence).unwrap();
        (section.samples, section.readiness, section.writes) =
            (self.evidence.samples.len() as u64, self.evidence.readiness.len() as u64, self.evidence.writes.len() as u64);
        self
    }
}

#[test]
fn a_coherent_scenario_passes_every_check() {
    let scenario = Scenario::new();
    assert_eq!(scenario.check(), Ok(()));
}

#[test]
fn every_control_is_refused_by_its_named_check() {
    for control in Control::ALL.into_iter().filter(|control| *control != Control::CanonicalInput) {
        let Expected::Application(check) = control.expected() else { panic!("{control:?} expects an application refusal") };
        assert_eq!(Scenario::new().control(control).refusal(), Some(check), "{control:?}");
    }
}

#[test]
fn the_canonical_input_control_changes_the_update_identity() {
    let scenario = Scenario::new();
    let update = scenario.receipt.identity.update.clone();
    let controlled = Scenario::new().control(Control::CanonicalInput);
    assert_ne!(controlled.receipt.identity.update, update);
    assert_eq!(Control::CanonicalInput.expected(), Expected::Identity);
    assert_eq!(controlled.check(), Ok(()));
}

#[test]
fn the_fifteen_controls_have_distinct_names() {
    let names: BTreeSet<&str> = Control::ALL.iter().map(|control| control.name()).collect();
    assert_eq!(names.len(), 15);
}

#[test]
fn a_view_whose_spelling_differs_is_a_mapping_refusal() {
    let mut scenario = Scenario::new();
    scenario.evidence.samples[5].task[0].spellings[0][1].0[0] ^= 1;
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::Mapping));
}

#[test]
fn a_code_word_other_than_the_contracts_is_a_code_identity_refusal() {
    let mut scenario = Scenario::new();
    let code = &mut scenario.evidence.samples[3].code;
    code.physical.0[0] ^= 1;
    for spelling in code.spellings.iter_mut().flatten() {
        spelling.0[0] ^= 1;
    }
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::CodeIdentity));
}

#[test]
fn a_task1_sample_outside_its_stack_is_a_task_context_refusal() {
    let mut scenario = Scenario::new();
    let index = scenario.evidence.samples.iter().position(|sample| sample.site == SiteRef::Application(SiteLabel::of(Pc(0xc22f84)))).unwrap();
    scenario.evidence.samples[index].registers.sp = Word(STACK_END + 8);
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::TaskContext));
}

#[test]
fn a_scope_site_missing_from_the_census_is_a_stack_context_refusal() {
    let mut scenario = Scenario::new();
    scenario.receipt.application.as_mut().unwrap().scope.as_mut().unwrap().sites.pop();
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::StackContext));
}

#[test]
fn a_nonzero_controller_state_at_the_reset_is_an_initializer_refusal() {
    let mut scenario = Scenario::new();
    let index = scenario.evidence.samples.iter().position(|sample| sample.site == SiteRef::Application(SiteLabel::of(Pc(0xc22f74)))).unwrap();
    let view = scenario.evidence.samples[index].state.iter_mut().find(|view| view.address == Guest(0x1467588)).unwrap();
    view.physical.0[0] = 1;
    for spelling in view.spellings.iter_mut().flatten() {
        spelling.0[0] = 1;
    }
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::Initializer));
}

#[test]
fn a_missing_or_later_first_ready_is_a_readiness_refusal() {
    let mut scenario = Scenario::new();
    scenario.receipt.application.as_mut().unwrap().first_ready = None;
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::Readiness));
    let mut scenario = Scenario::new();
    scenario.receipt.application.as_mut().unwrap().first_ready.as_mut().unwrap().instructions.0 += 1;
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::Readiness));
}

#[test]
fn a_readiness_probe_that_differs_from_its_views_is_a_readiness_refusal() {
    let mut scenario = Scenario::new();
    let row = &mut scenario.evidence.readiness[0].tasks.rows[0];
    for bytes in std::iter::once(&mut row.physical).chain(row.spellings.iter_mut().flatten()) {
        bytes.0[12] = 0x11;
    }
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::Readiness));
}

#[test]
fn an_unstable_candidate_is_retried_on_a_later_receive() {
    let scenario = Scenario::with(&[Plan::Unstable, Plan::Stable]);
    assert_eq!(scenario.check(), Ok(()));
    let first_receive = scenario.evidence.readiness[0].receive;
    assert!(scenario.section().first_ready.unwrap().receive > first_receive);
    let mut earlier = Scenario::with(&[Plan::Unstable, Plan::Stable]);
    let candidate = earlier.evidence.readiness[0].clone();
    earlier.receipt.application.as_mut().unwrap().first_ready =
        Some(FirstReady { readiness: candidate.index, receive: candidate.receive, instructions: candidate.instructions });
    assert_eq!(earlier.refusal(), Some(ApplicationCheck::Readiness));
}

#[test]
fn a_candidate_whose_interval_passes_the_search_end_is_not_first_ready() {
    let scenario = Scenario::with(&[Plan::PastEnd, Plan::Quiet]);
    assert!(scenario.section().first_ready.is_none());
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::Readiness));
    let mut recorded = Scenario::with(&[Plan::PastEnd, Plan::Quiet]);
    let candidate = recorded.evidence.readiness[0].clone();
    recorded.receipt.application.as_mut().unwrap().first_ready =
        Some(FirstReady { readiness: candidate.index, receive: candidate.receive, instructions: candidate.instructions });
    assert_eq!(recorded.refusal(), Some(ApplicationCheck::Readiness));
}

#[test]
fn a_settle_end_off_the_interval_or_a_readiness_gap_is_a_readiness_refusal() {
    let mut late = Scenario::new();
    let end = late.evidence.readiness.iter().position(|sample| sample.kind == ReadinessKind::SettleEnd).unwrap();
    late.evidence.readiness[end].instructions.0 += 1;
    assert_eq!(late.refusal(), Some(ApplicationCheck::Readiness));
    let mut gap = Scenario::new();
    gap.evidence.readiness.remove(1);
    gap.evidence.readiness.remove(1);
    for (index, sample) in gap.evidence.readiness.iter_mut().enumerate() {
        sample.index = ReadinessIndex(index as u64);
    }
    assert_eq!(gap.refusal(), Some(ApplicationCheck::Readiness));
}

#[test]
fn a_release_under_another_packet_is_a_queue_provenance_refusal() {
    let mut scenario = Scenario::new();
    let index = scenario.evidence.samples.iter().position(|sample| sample.site == SiteRef::Queue(QueueLabel::of(Pc(0x725414)))).unwrap();
    scenario.evidence.samples[index].registers.r[1].0 += 4;
    let target = scenario.evidence.samples[index + 1].registers.r[1].0 + 4;
    scenario.evidence.samples[index + 1].registers.r[1] = Word(target);
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::QueueProvenance));
}

#[test]
fn a_skipped_hook_is_a_hook_sequence_refusal() {
    let mut scenario = Scenario::new();
    let index = scenario.evidence.samples.iter().position(|sample| sample.site == SiteRef::Application(SiteLabel::of(Pc(0xbfb20c)))).unwrap();
    scenario.evidence.samples[index].registers.lr = Word(0);
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::HookSequence));
}

#[test]
fn an_unchanged_frame_after_an_ordinary_input_is_a_render_completion_refusal() {
    let mut scenario = Scenario::new();
    let section = scenario.receipt.application.as_mut().unwrap();
    section.visible[1].png = section.visible[0].png.clone();
    assert!(matches!(scenario.refusal(), Some(ApplicationCheck::RenderCompletion)));
}

#[test]
fn a_reordered_route_is_a_route_refusal() {
    let mut scenario = Scenario::new();
    scenario.receipt.application.as_mut().unwrap().inputs.swap(0, 12);
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::Route));
}

#[test]
fn a_count_that_differs_from_the_evidence_is_a_capture_bound_refusal() {
    let mut scenario = Scenario::new();
    scenario.receipt.application.as_mut().unwrap().writes += 1;
    assert_eq!(scenario.refusal(), Some(ApplicationCheck::CaptureBound));
}

#[test]
fn a_changed_evidence_file_is_a_capture_bound_refusal() {
    let scenario = Scenario::new();
    let section = scenario.section();
    std::fs::write(scenario.directory.join(EVIDENCE), b"changed").unwrap();
    let refusal = fujimod_xm1::application_observation::check_role(scenario.contract.application().unwrap(), Role::Composed, section, &scenario.directory);
    assert_eq!(refusal.map_err(|refusal| refusal.check), Err(ApplicationCheck::CaptureBound));
}

#[test]
fn the_pair_check_compares_inputs_visible_state_and_targets() {
    let (retained, owned) = (Scenario::new(), Scenario::new());
    assert_eq!(check_pair(retained.section(), owned.section()), Ok(()));
    let mut differing = owned.section().clone();
    differing.inputs[15].key = RouteKey::Right;
    assert_eq!(check_pair(retained.section(), &differing), Err(PairField::Inputs));
    let mut differing = owned.section().clone();
    differing.visible[2].menu = None;
    assert_eq!(check_pair(retained.section(), &differing), Err(PairField::Visible));
    let mut differing = owned.section().clone();
    differing.targets[0].word = CodeWord(0);
    assert_eq!(check_pair(retained.section(), &differing), Err(PairField::Targets));
}

#[test]
fn evidence_and_a_schema_five_receipt_round_trip() {
    let scenario = Scenario::new();
    assert_eq!(read_evidence(&scenario.directory, &scenario.section().evidence).unwrap(), scenario.evidence);
    let bytes = serde_json::to_vec(&scenario.receipt).unwrap();
    let parsed: Receipt = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed, scenario.receipt);
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap()["schema"], json!(5));
}

#[test]
fn a_schema_four_receipt_serializes_without_an_application_section() {
    let mut receipt = Scenario::new().receipt;
    receipt.schema = Schema::Four;
    receipt.application = None;
    let value = serde_json::to_value(&receipt).unwrap();
    assert!(value.get("application").is_none());
    assert_eq!(serde_json::from_value::<Receipt>(value).unwrap(), receipt);
}

#[test]
fn a_code_range_label_names_its_range() {
    let contract = parse(&schema5()).unwrap();
    let pump = contract.application().unwrap().code_range(CodeLabel::Pump).unwrap();
    assert_eq!((pump.start, pump.end), (u64::from(PUMP), u64::from(PUMP + 32)));
}
