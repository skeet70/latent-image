use fujimod_emulator::board::observation::{WatchKinds, WatchRange};
use fujimod_emulator_devices::{Cx, Device, Host, Record};
use fujimod_emulator_exr2_system::timer::{self, Options, SystemTimer, Timers, Wiring};
use fujimod_xm1::attach_relations::{
    Access, AttachSources, LogWrite, LzRegisters, RelationFailure, TimerState, Token, TransactionRow, TransactionWriter,
};
use fujimod_xm1::attach_state::{
    self, AllowanceLabel, AttachRuleRefusal, AttachState, AttachVerdict, BoardLeaf, BusAddress, CaptureRegion, ContextRegister, CoreContext,
    Occupant, ProcessorAddress, ReadPath, RegionRead, Space, SpaceSpan, Span, StackCheck, UnusedEntry,
};
use fujimod_xm1::composed_catalog::{
    Bank, CatalogRefusal, Contract, Core, DeclarationProblem, GuardBound, Label, Pc, PiecePolicy, PieceKind, PlanSpan,
    RangePlan, Role, Sha256, Target, WritesBasis,
};
use fujimod_xm1::composed_receipt::{
    Admission, ByteRange, CaptureDirectories, CarrierField, CarrierWrites, DeclaredUse, Difference, GuardObservation, GuardState,
    HexBytes, Identity, IdentityField, LabelCount, MenuObservation, Outcome, ReadCapture, Receipt, ReceiptKind, RecordedPolicy,
    Register, Row, SchemaFour, Verdict, VerdictKind, WatchRow, Word, WriteKind, Writer, WriterCount, carrier_writes, compare,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

const FIXTURE: &str = include_str!("data/composed-contract-v4.json");
const END: u64 = 1 << 32;

fn fixture() -> Value { serde_json::from_str(FIXTURE).unwrap() }

fn parse(value: &Value) -> Result<Contract, CatalogRefusal> { Contract::parse(&serde_json::to_vec(value).unwrap()) }

fn label(text: &str) -> Label { Label::try_from(text.to_owned()).unwrap() }

fn refused(edit: impl FnOnce(&mut Value)) -> CatalogRefusal {
    let mut value = fixture();
    edit(&mut value);
    parse(&value).unwrap_err()
}

#[test]
fn fixture_parses_into_role_site_plans() {
    let contract = parse(&fixture()).unwrap();
    let composed = contract.sites(Role::Composed);
    assert_eq!(composed.sites, BTreeMap::from([
        (Pc(24576), label("first-stage/entry")),
        (Pc(24580), label("first-stage/handoff")),
        (Pc(263216), label("primary-publisher/entry-core0")),
        (Pc(264000), label("primary-publisher/return-core0")),
    ]));
    assert_eq!(composed.stops.iter().cloned().collect::<Vec<_>>(),
        [label("first-stage/handoff"), label("primary-publisher/entry-core0"), label("primary-publisher/return-core0")]);
    let retained = contract.sites(Role::Retained);
    assert_eq!(retained.sites, BTreeMap::from([
        (Pc(6144), label("first-stage/handoff")),
        (Pc(263216), label("primary-publisher/entry-core0")),
        (Pc(264000), label("primary-publisher/return-core0")),
    ]));
    assert_eq!(retained.trace_sites().get(&6144).map(String::as_str), Some("first-stage/handoff"));
    assert_eq!(contract.watch_classes(), [
        WatchRange { start: 24576, end: 25772, kinds: WatchKinds::WRITES },
        WatchRange { start: 263216, end: 264620, kinds: WatchKinds::WRITES },
    ]);
}

#[test]
fn unknown_fields_refuse_at_every_level() {
    assert!(matches!(refused(|v| v["extra"] = json!(1)), CatalogRefusal::Shape(_)));
    assert!(matches!(refused(|v| v["pieces"][0]["catalog"]["boundaries"][0]["extra"] = json!(1)), CatalogRefusal::Shape(_)));
    assert!(matches!(refused(|v| v["pieces"][1]["catalog"]["writes"]["allow"][0]["extra"] = json!(1)), CatalogRefusal::Shape(_)));
    assert!(matches!(refused(|v| v["canonical"]["targets"]["image6"] = v["canonical"]["targets"]["image5"].clone()), CatalogRefusal::Shape(_)));
    assert!(matches!(refused(|v| { v["pieces"][0].as_object_mut().unwrap().remove("premises"); }), CatalogRefusal::Shape(_)));
}

#[test]
fn unsorted_and_overlapping_spans_refuse() {
    let owned = |v: &mut Value, spans: Value| v["pieces"][0]["catalog"]["owned_code"] = spans;
    assert!(matches!(refused(|v| owned(v, json!([{"start": 25000, "end": 25772}, {"start": 24576, "end": 24600}]))),
        CatalogRefusal::UnsortedSpans(_)));
    assert!(matches!(refused(|v| owned(v, json!([{"start": 24576, "end": 25000}, {"start": 24900, "end": 25772}]))),
        CatalogRefusal::OverlappingSpans(_)));
    assert!(matches!(refused(|v| v["pieces"][0]["carriers"] = json!([
        {"target": "record2", "start": 0, "end": 4}, {"target": "record1", "start": 24576, "end": 25772}])),
        CatalogRefusal::UnsortedSpans(_)));
    assert!(matches!(refused(|v| v["joins"][0]["spans"] = json!([
        {"target": "record2", "start": 66560, "end": 66564}, {"target": "record2", "start": 66562, "end": 66570}])),
        CatalogRefusal::OverlappingSpans(_)));
}

#[test]
fn duplicate_labels_and_pcs_refuse() {
    assert_eq!(refused(|v| v["pieces"][0]["catalog"]["boundaries"][1]["label"] = json!("first-stage/entry")),
        CatalogRefusal::DuplicateLabel(label("first-stage/entry")));
    assert_eq!(refused(|v| v["pieces"][1]["catalog"]["boundaries"][1]["composed_pc"] = json!(263216)),
        CatalogRefusal::DuplicatePc { role: Role::Composed, pc: Pc(263216) });
    assert_eq!(refused(|v| v["pieces"][0]["catalog"]["boundaries"][1]["retained_pc"] = json!(264000)),
        CatalogRefusal::DuplicatePc { role: Role::Retained, pc: Pc(264000) });
    assert_eq!(refused(|v| v["pieces"][0]["catalog"]["boundaries"][0]["label"] = json!("primary-publisher/entry")),
        CatalogRefusal::UnqualifiedLabel(label("primary-publisher/entry")));
}

#[test]
fn owned_entry_and_site_pcs_must_lie_in_owned_code_but_returns_need_not() {
    assert_eq!(refused(|v| v["pieces"][1]["catalog"]["boundaries"][0]["composed_pc"] = json!(264000 - 4)),
        CatalogRefusal::OwnedPcOutsideCode(label("primary-publisher/entry-core0")));
    assert_eq!(refused(|v| v["pieces"][0]["catalog"]["boundaries"][1]["composed_pc"] = json!(25772)),
        CatalogRefusal::OwnedPcOutsideCode(label("first-stage/handoff")));
    let mut value = fixture();
    value["pieces"][1]["catalog"]["boundaries"][1]["composed_pc"] = json!(0x70000);
    assert!(parse(&value).is_ok());
}

#[test]
fn guard_attach_must_name_a_stop_boundary_armed_in_both_roles() {
    assert_eq!(refused(|v| v["pieces"][1]["catalog"]["boundaries"][0]["stop"] = json!(false)),
        CatalogRefusal::AttachNotStop(label("primary-publisher/entry-core0")));
    assert_eq!(refused(|v| v["guard"]["attach"] = json!("first-stage/missing")),
        CatalogRefusal::AttachUnknown(label("first-stage/missing")));
    assert_eq!(refused(|v| {
        v["guard"]["attach"] = json!("first-stage/entry");
        v["pieces"][0]["catalog"]["boundaries"][0]["stop"] = json!(true);
    }), CatalogRefusal::AttachNotRetained(label("first-stage/entry")));
}

#[test]
fn write_allow_overlap_caps_aliases_and_foreign_tcm_refuse() {
    let allow = |v: &mut Value, ranges: Value| v["pieces"][1]["catalog"]["writes"]["allow"] = ranges;
    assert!(matches!(refused(|v| allow(v, json!([
        {"target": "physical", "start": 318176, "end": 323852}, {"target": "physical", "start": 323000, "end": 330000}]))),
        CatalogRefusal::OverlappingSpans(_)));
    assert!(matches!(refused(|v| allow(v, json!([
        {"target": "physical", "start": 318176, "end": 323852}, {"target": "physical", "start": 0x40000000u64 + 318180, "end": 0x40000000u64 + 318200}]))),
        CatalogRefusal::OverlappingSpans(_)));
    assert!(matches!(refused(|v| allow(v, json!([{"target": "physical", "start": 0x3ffffff0u64, "end": 0x40000010u64}]))),
        CatalogRefusal::AliasStraddle(_)));
    assert!(matches!(refused(|v| allow(v, json!([{"target": "tcm", "core": 1, "bank": "data", "start": 0, "end": 16}]))),
        CatalogRefusal::ForeignTcm { core: Core::Secondary, .. }));
    assert!(matches!(refused(|v| allow(v, json!([{"target": "physical", "bank": "data", "start": 0, "end": 16}]))),
        CatalogRefusal::WriteRangeShape(_)));
    assert!(matches!(refused(|v| allow(v, json!([{"target": "physical", "core": 1, "start": 0, "end": 16}]))),
        CatalogRefusal::ForeignTcm { core: Core::Secondary, .. }));
    let many: Vec<Value> = (0..520u64).map(|index| json!({"target": "physical", "start": 0x100000 + 16 * index, "end": 0x100000 + 16 * index + 8})).collect();
    assert!(matches!(refused(|v| allow(v, Value::Array(many))), CatalogRefusal::GuardBound(GuardBound::Ranges(_))));
    let spans: Vec<Value> = (0..130u64).map(|index| json!({"start": 263216 + 8 * index, "end": 263216 + 8 * index + 4})).collect();
    assert_eq!(refused(|v| v["pieces"][1]["catalog"]["owned_code"] = Value::Array(spans)),
        CatalogRefusal::GuardBound(GuardBound::PcSpans(524)));
    let mut value = fixture();
    value["pieces"][1]["catalog"]["boundaries"][1]["cores"] = json!([0, 1]);
    allow(&mut value, json!([{"target": "tcm", "core": 1, "bank": "data", "start": 0, "end": 16}]));
    assert!(parse(&value).is_ok());
}

#[test]
fn core_sets_and_write_bases_refuse_when_malformed() {
    for cores in [json!([]), json!([1, 0]), json!([0, 0]), json!([2])] {
        assert!(matches!(refused(|v| v["pieces"][1]["catalog"]["boundaries"][1]["cores"] = cores), CatalogRefusal::Shape(_)));
    }
    assert_eq!(refused(|v| v["pieces"][1]["catalog"]["boundaries"][0]["cores"] = json!([0, 1])),
        CatalogRefusal::AttachCores(label("primary-publisher/entry-core0")));
    assert!(matches!(refused(|v| v["pieces"][0]["catalog"]["writes"]["premise"] = Value::Null), CatalogRefusal::WritesPremise(_)));
    assert!(matches!(refused(|v| v["pieces"][0]["catalog"]["writes"]["basis"] = json!("proved")), CatalogRefusal::WritesPremise(_)));
    assert!(matches!(refused(|v| v["pieces"][0]["catalog"]["writes"] = json!({"allow": [], "basis": "unenumerated", "premise": "p"})),
        CatalogRefusal::Shape(_)));
}

#[test]
fn declarations_parse_per_role_and_refuse_when_malformed() {
    let contract = parse(&fixture()).unwrap();
    assert_eq!(contract.declared(Role::Retained).len(), 2);
    assert_eq!(contract.guard_config(Role::Composed).declared, contract.declared(Role::Composed).iter().map(|d| d.guard()).collect::<Vec<_>>());
    assert_eq!(contract.plan().roles[1].declared, contract.declared(Role::Composed));
    fn tcm(v: &mut Value) -> &mut Value { &mut v["guard"]["declared"]["composed"][1] }
    let refusal = |problem| CatalogRefusal::Declaration { role: Role::Composed, problem };
    assert_eq!(refused(|v| tcm(v)["next"] = json!(4294934545u64)), refusal(DeclarationProblem::Unchanged(1)));
    assert_eq!(refused(|v| tcm(v)["writer"] = json!(24670)), refusal(DeclarationProblem::UnalignedWriter(1)));
    assert_eq!(refused(|v| {
        let copy = tcm(v).clone();
        v["guard"]["declared"]["composed"].as_array_mut().unwrap().push(copy);
    }), refusal(DeclarationProblem::SharedPrevious(2)));
    assert!(matches!(refused(|v| tcm(v)["extra"] = json!(1)), CatalogRefusal::Shape(_)));
    assert!(matches!(refused(|v| tcm(v)["bank"] = json!("other")), CatalogRefusal::Shape(_)));
    assert!(matches!(refused(|v| { v["guard"].as_object_mut().unwrap().remove("declared"); }), CatalogRefusal::Shape(_)));
    let many: Vec<Value> = (0..17u32).map(|index| json!({
        "kind": "tcm-region", "core": 0, "bank": "data", "previous": index, "next": index + 1, "writer": 4 * index, "citation": "c",
    })).collect();
    assert_eq!(refused(|v| v["guard"]["declared"]["retained"] = Value::Array(many)), CatalogRefusal::GuardBound(GuardBound::Declared(17)));
    assert!(matches!(refused(|v| v["schema"] = json!(3)), CatalogRefusal::Shape(_)));
}

#[test]
fn owned_code_shared_by_two_pieces_refuses_in_either_spelling() {
    let overlap = CatalogRefusal::OwnedCodeOverlap {
        first: "first-stage".to_owned().try_into().unwrap(), second: "primary-publisher".to_owned().try_into().unwrap(),
    };
    assert_eq!(refused(|v| v["pieces"][1]["catalog"]["owned_code"] = json!([
        {"start": 25000, "end": 25100}, {"start": 263216, "end": 263924}])), overlap);
    assert_eq!(refused(|v| v["pieces"][1]["catalog"]["owned_code"] = json!([
        {"start": 263216, "end": 263924}, {"start": 0x40000000u64 + 25000, "end": 0x40000000u64 + 25100}])), overlap);
    assert!(matches!(refused(|v| v["pieces"][0]["catalog"]["owned_code"] = json!([{"start": 0x3ffff000u64, "end": 0x40001000u64}])),
        CatalogRefusal::AliasStraddle(_)));
}

#[test]
fn reads_dats_and_boundary_counts_refuse() {
    assert_eq!(refused(|v| v["pieces"][0]["catalog"]["reads"][0]["boundary"] = json!("first-stage/entry")),
        CatalogRefusal::ReadBoundary(label("first-stage/peer-cells")));
    assert_eq!(refused(|v| {
        v["pieces"][0]["catalog"]["boundaries"][0]["stop"] = json!(true);
        v["pieces"][0]["catalog"]["reads"][0]["boundary"] = json!("first-stage/entry");
    }), CatalogRefusal::ReadNotRetained(label("first-stage/peer-cells")));
    let mut value = fixture();
    value["pieces"][0]["catalog"]["boundaries"][0]["stop"] = json!(true);
    value["pieces"][0]["catalog"]["reads"][0]["boundary"] = json!("first-stage/entry");
    value["pieces"][0]["catalog"]["reads"][0]["differs"] = json!("retained has no first-stage entry");
    assert!(parse(&value).is_ok());
    assert_eq!(refused(|v| v["composed"]["dat_sha256"] = v["canonical"]["dat_sha256"].clone()), CatalogRefusal::IdenticalDats);
    assert_eq!(refused(|v| {
        let boundaries = v["pieces"][1]["catalog"]["boundaries"].as_array_mut().unwrap();
        boundaries.extend((0..4097u64).map(|index| json!({
            "label": format!("primary-publisher/return-{index}"), "kind": "return", "cores": [0],
            "composed_pc": 0x100000 + 4 * index, "retained_pc": null, "stop": false, "compare": "presence", "record_limit": 1,
        })));
    }), CatalogRefusal::BoundaryBound { role: Role::Composed, core: Core::Primary, count: 4101 });
}

const ALIAS: u64 = 0x4000_0000;

fn plan_span(start: u64, end: u64) -> PlanSpan { PlanSpan { start, end } }

fn spellings(start: u64, end: u64) -> Vec<PlanSpan> { vec![plan_span(start, end), plan_span(ALIAS + start, ALIAS + end)] }

fn first_stage() -> PieceKind { "first-stage".to_owned().try_into().unwrap() }

fn publisher() -> PieceKind { "primary-publisher".to_owned().try_into().unwrap() }

fn policy_on(policies: &[PiecePolicy], piece: &PieceKind, core: Core) -> PiecePolicy {
    policies.iter().find(|policy| &policy.piece == piece && policy.core == core).unwrap().clone()
}

fn physical_plan(start: u64, end: u64) -> RangePlan { RangePlan::Physical { start, end } }

#[test]
fn each_piece_gets_its_own_partition_on_every_core() {
    let contract = parse(&fixture()).unwrap();
    let policies = contract.policies(Role::Composed);
    assert_eq!(policies.len(), 4);
    let config = contract.guard_config(Role::Composed);
    let first = policy_on(&policies, &first_stage(), Core::Primary);
    assert_eq!((first.basis, first.spans.clone()), (WritesBasis::Premised, spellings(24576, 25772)));
    assert_eq!(first.allow, [physical_plan(17152, 21120), RangePlan::Tcm { core: 0, bank: Bank::Instruction, start: 0, end: 8192 }]);
    assert!(policy_on(&policies, &first_stage(), Core::Secondary).allow.is_empty());
    for core in Core::ALL {
        let enumerated = policy_on(&policies, &publisher(), core);
        assert_eq!(enumerated.spans, spellings(263216, 263924));
        assert_eq!(enumerated.allow, [physical_plan(318176, 323852), physical_plan(404564, 406624)]);
        assert_eq!(config.sources[core.index()].partitions.len(), 2);
        assert!(config.sources[core.index()].partitions.iter().all(|partition| partition.destinations.forbid.is_empty()));
    }
    assert!(contract.policies(Role::Retained).is_empty());
    assert!(contract.guard_config(Role::Retained).sources.iter().all(|source| source.partitions.is_empty()));
}

#[test]
fn a_piece_allow_never_reaches_another_piece_partition() {
    let mut value = fixture();
    value["pieces"][0]["catalog"]["writes"] = json!({
        "allow": [{"target": "physical", "start": 0, "end": 24576}, {"target": "tcm", "core": 0, "bank": "instruction", "start": 0, "end": 65536}],
        "basis": "premised", "premise": "first-stage scratch",
    });
    let policies = parse(&value).unwrap().policies(Role::Composed);
    assert_eq!(policy_on(&policies, &first_stage(), Core::Primary).allow,
        [physical_plan(0, 24576), RangePlan::Tcm { core: 0, bank: Bank::Instruction, start: 0, end: 65536 }]);
    assert_eq!(policy_on(&policies, &first_stage(), Core::Secondary).allow, [physical_plan(0, 24576)]);
    for core in Core::ALL {
        assert_eq!(policy_on(&policies, &publisher(), core).allow, [physical_plan(318176, 323852), physical_plan(404564, 406624)]);
    }
}

#[test]
fn a_physical_range_with_a_core_binds_only_that_core() {
    let mut value = fixture();
    value["pieces"][1]["catalog"]["boundaries"][1]["cores"] = json!([0, 1]);
    value["pieces"][1]["catalog"]["writes"]["allow"] = json!([
        {"target": "physical", "start": 318176, "end": 323852},
        {"target": "physical", "core": 0, "start": 404564, "end": 406624},
        {"target": "physical", "core": 1, "start": 404564, "end": 406600},
    ]);
    let policies = parse(&value).unwrap().policies(Role::Composed);
    assert_eq!(policy_on(&policies, &publisher(), Core::Primary).allow, [physical_plan(318176, 323852), physical_plan(404564, 406624)]);
    assert_eq!(policy_on(&policies, &publisher(), Core::Secondary).allow, [physical_plan(318176, 323852), physical_plan(404564, 406600)]);
    value["pieces"][1]["catalog"]["writes"]["allow"] = json!([
        {"target": "physical", "start": 318176, "end": 323852},
        {"target": "physical", "core": 1, "start": 320000, "end": 320004},
    ]);
    assert!(matches!(parse(&value), Err(CatalogRefusal::OverlappingSpans(_))));
}

#[test]
fn guard_config_translates_alias_destinations_to_physical() {
    let mut value = fixture();
    value["pieces"][1]["catalog"]["writes"]["allow"] = json!([
        {"target": "physical", "start": 318176, "end": 323852},
        {"target": "physical", "start": ALIAS + 404564, "end": ALIAS + 406624},
    ]);
    let policies = parse(&value).unwrap().policies(Role::Composed);
    assert_eq!(policy_on(&policies, &publisher(), Core::Primary).allow, [physical_plan(318176, 323852), physical_plan(404564, 406624)]);
}

#[test]
fn an_owned_span_already_listed_in_both_spellings_yields_the_same_policy() {
    let mut value = fixture();
    value["pieces"][1]["catalog"]["owned_code"] = json!([
        {"start": 263216, "end": 263924}, {"start": ALIAS + 263216, "end": ALIAS + 263924}]);
    assert_eq!(parse(&value).unwrap().policies(Role::Composed), parse(&fixture()).unwrap().policies(Role::Composed));
}

fn row(text: &str, registers: [u32; 4], sp: u32, reads: Vec<ReadCapture>) -> Row {
    let w = Word;
    Row {
        label: label(text), core: Core::Primary, total_instructions: 1000,
        r0: w(registers[0]), r1: w(registers[1]), r2: w(registers[2]), r3: w(registers[3]),
        r4: w(4), r5: w(5), r6: w(6), r7: w(7), r8: w(8), r9: w(9), r10: w(10), r11: w(11), r12: w(12),
        sp: w(sp), lr: w(0x14), cpsr: w(0x1d3), reads,
    }
}

fn digest(byte: char) -> Sha256 { Sha256::try_from(byte.to_string().repeat(64)).unwrap() }

fn receipt(contract: &Contract, contract_digest: &Sha256, role: Role) -> Receipt {
    let read = |name: &str, address: u32, bytes: Vec<u8>| ReadCapture { label: label(name), address, bytes: HexBytes(bytes) };
    let rows = vec![
        row("first-stage/handoff", [1, 2, 3, 4], 0x63400, vec![read("first-stage/peer-cells", 1073872896, vec![1; 8])]),
        row("primary-publisher/entry-core0", [0x10, 0x11, 0x12, 0x13], 0x63420, vec![]),
        row("primary-publisher/return-core0", [0, 0x99, 0x98, 0x97], 0x63420, vec![read("primary-publisher/table-a", 1074059992, vec![7; 4])]),
    ];
    let counts = rows.iter().map(|row| LabelCount { label: row.label.clone(), core: Core::Primary, total: 1 }).collect();
    Receipt {
        schema: SchemaFour,
        kind: ReceiptKind::ComposedStartupReceipt,
        role,
        identity: Identity {
            run: digest('a'), update: contract.identities(role).dat_sha256.clone(), boot_rom: digest('b'),
            observer: digest('c'), contract: contract_digest.clone(),
        },
        policies: contract.policies(role).into_iter().map(|policy| RecordedPolicy { policy, checks_before_boundary: 1, checks: 3 }).collect(),
        outcome: Outcome::Observed,
        error: None,
        machine_failure: None,
        instructions: 80_000_000,
        rows,
        counts,
        off_core: vec![],
        attach: None,
        attach_sources: None,
        guard: Some(GuardObservation {
            boundary_instructions: 900, boundary_row: 0, end_instructions: 250_000_000, status: GuardState::Ended, first_fault: None,
            write_checks: 12, mutation_attempts: 0, mutations_admitted: 2,
            declared: contract.declared(role).iter().map(|declaration| DeclaredUse {
                declaration: declaration.clone(), admitted: Some(Admission { core: Core::Primary, instructions: Some(100) }),
            }).collect(),
            carrier_writes_before_boundary: carriers(contract, &[]), carrier_writes: carriers(contract, &stock_writes()),
            watch_violation: None, watch_end_error: None,
        }),
        menu: MenuObservation { erased_settings: true, shooting: true },
    }
}

/// One role's synthetic capture: nonzero bytes by (space, address), the board state, both cores and relation sources.
#[derive(Clone)]
struct Spec {
    bytes: BTreeMap<(Space, u64), u8>,
    board: Value,
    cores: [CoreContext; 2],
    sources: Option<SourceSpec>,
}

#[derive(Clone)]
struct SourceSpec {
    reset: SystemTimer,
    rows: Vec<TransactionRow>,
    log_writes: Vec<LogWrite>,
}

fn write_sources(directory: &Path, sources: &SourceSpec) -> AttachSources {
    let mut writer = TransactionWriter::create(directory).unwrap();
    for row in &sources.rows {
        writer.push(row).unwrap();
    }
    AttachSources { timer_reset: TimerState(sources.reset.clone()), transactions: writer.finish().unwrap(), log_writes: sources.log_writes.clone() }
}

/// Enabled, 8 KiB at 0x70000000.
const ATCM: u32 = 0x7000_0011;

fn context(core: Core, pc: u32, sp: u32, cpsr: u32) -> CoreContext {
    CoreContext {
        core, r: [Word(0); 13], sp: Word(sp), lr: Word(0x409e0), pc: Word(pc), cpsr: Word(cpsr), spsr: Word(0), cpacr: Word(0),
        fpexc: Word(0), fpscr: Word(0), sctlr: Word(0xc5187d), tcm: [Word(0), Word(ATCM)],
    }
}

fn board_state() -> Value {
    json!({
        "execution_mode": "strict", "execution_policy": "exact", "counts_as_boot": true, "timeline": {"now": 1}, "instructions": [1, 2],
        "released": [true, true], "waiting": [false, true], "waiting_event": [false, false], "events": [false, false],
        "next_core": 0, "slice_left": 3, "quantum": 1000, "failure": null, "memory_decoders": {"choices": [1], "pending": [null], "failure": null},
        "dma_mappings": [], "gic": {"pending": [0]}, "core_interrupt_devices": [null, null], "outputs": [], "implementation_cluster": 0,
        "unmodeled_system": [], "aprofile": [null, null], "mprofile": [null, null],
        "bus": {"mode": "strict", "unmodeled": [], "discovery_shadow": null, "continue_refusals": [], "frontier": [{"tick": 4, "pcs": [16]}],
            "devices": [{"kind": "timer", "windows": [], "native_dependencies": [], "state": {"count": 1, "mode": 0}}]},
        "discovery_callee_stubs": [],
    })
}

fn spec(role: Role) -> Spec {
    let peer_pc = match role { Role::Retained => 0xd0a44, Role::Composed => 0x10500 };
    Spec {
        bytes: BTreeMap::from([((Space::Physical, 0x100), 0x5a)]),
        board: board_state(),
        cores: [context(Core::Primary, 0x40430, 0x4006_3450, 0x6000_00d3), context(Core::Secondary, peer_pc, 0x4007_0000, 0x6000_00d3)],
        sources: None,
    }
}

fn scratch() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!("composed-startup-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

/// Physical `[0, 0x80000)` read through two bus stretches, core 0's ATCM, and physical `[0x90000, 0x91000)` unreachable.
fn write_capture(directory: &Path, spec: &Spec) -> AttachState {
    let bus = |start: u64, end: u64, address: u64| RegionRead { start, end, path: ReadPath::Bus { address: BusAddress(address) } };
    let atcm = ReadPath::Core { core: Core::Primary, address: ProcessorAddress(0) };
    let regions = [
        CaptureRegion { space: Space::Physical, start: 0, end: 0x80000, reads: vec![bus(0, 0x4000, 0x4000_0000), bus(0x4000, 0x80000, 0x4000)] },
        CaptureRegion { space: Space::Tcm { core: Core::Primary, bank: Bank::Instruction }, start: 0, end: 0x2000,
            reads: vec![RegionRead { start: 0, end: 0x2000, path: atcm }] },
    ];
    let (records, memory_sha256, memory_bytes) = attach_state::write_memory_snapshot(&directory.join(attach_state::MEMORY_FILE), &regions,
        |region, offset, buffer| {
            buffer.fill(0);
            for (index, byte) in buffer.iter_mut().enumerate() {
                if let Some(value) = spec.bytes.get(&(region.space, region.start + offset + index as u64)) {
                    *byte = *value;
                }
            }
            Ok(())
        }).unwrap();
    let (board_sha256, board_bytes) = attach_state::write_board_state(&directory.join(attach_state::BOARD_FILE), &spec.board).unwrap();
    AttachState {
        instructions: [5_000_000, 886_999],
        cores: spec.cores.clone(),
        regions: records,
        unreachable: vec![Span::new(0x90000, 0x91000).unwrap()],
        memory_file: attach_state::FileRecord { path: attach_state::MEMORY_FILE.into(), sha256: memory_sha256, compressed_bytes: memory_bytes },
        board_file: attach_state::FileRecord { path: attach_state::BOARD_FILE.into(), sha256: board_sha256, compressed_bytes: board_bytes },
    }
}

/// Writes each role's capture from `spec(role)`, the composed one after `edit_owned`.
fn full_compare_with(contract: &Contract, digest: &Sha256, retained: Receipt, owned: Receipt, edit_owned: impl FnOnce(&mut Spec),
    after: impl FnOnce(&Path, &mut Receipt)) -> Verdict {
    let mut owned_spec = spec(Role::Composed);
    edit_owned(&mut owned_spec);
    compare_specs(contract, digest, retained, owned, &spec(Role::Retained), &owned_spec, after)
}

fn compare_specs(contract: &Contract, digest: &Sha256, mut retained: Receipt, mut owned: Receipt, retained_spec: &Spec, owned_spec: &Spec,
    after: impl FnOnce(&Path, &mut Receipt)) -> Verdict {
    let (retained_directory, owned_directory) = (scratch(), scratch());
    for (receipt, directory, spec) in [(&mut retained, &retained_directory, retained_spec), (&mut owned, &owned_directory, owned_spec)] {
        receipt.attach = Some(write_capture(directory, spec));
        receipt.attach_sources = spec.sources.as_ref().map(|sources| write_sources(directory, sources));
    }
    after(&owned_directory, &mut owned);
    let roundtrip: Receipt = serde_json::from_slice(&serde_json::to_vec(&owned).unwrap()).unwrap();
    let result = compare(contract, digest, &retained, &roundtrip, &CaptureDirectories { retained: &retained_directory, owned: &owned_directory });
    for directory in [retained_directory, owned_directory] {
        std::fs::remove_dir_all(directory).unwrap();
    }
    assert_eq!(result.verdict == VerdictKind::Equal, result.first_difference.is_none());
    result
}

fn compare_with(contract: &Contract, digest: &Sha256, retained: Receipt, owned: Receipt, edit_owned: impl FnOnce(&mut Spec),
    after: impl FnOnce(&Path, &mut Receipt)) -> (Option<Difference>, Option<AttachVerdict>) {
    let result = full_compare_with(contract, digest, retained, owned, edit_owned, after);
    (result.first_difference, result.attach)
}

fn attach_report(value: &Value, edit_owned: impl FnOnce(&mut Spec)) -> Verdict {
    let bytes = serde_json::to_vec(value).unwrap();
    let contract = Contract::parse(&bytes).unwrap();
    let digest = Sha256::of(&bytes);
    let (retained, owned) = (receipt(&contract, &digest, Role::Retained), receipt(&contract, &digest, Role::Composed));
    full_compare_with(&contract, &digest, retained, owned, edit_owned, |_, _| {})
}

fn attach_verdict(value: &Value, edit_owned: impl FnOnce(&mut Spec)) -> (Option<Difference>, Option<AttachVerdict>) {
    let result = attach_report(value, edit_owned);
    (result.first_difference, result.attach)
}

fn verdict(value: &Value, edit: impl FnOnce(&mut Receipt)) -> Option<Difference> {
    let bytes = serde_json::to_vec(value).unwrap();
    let contract = Contract::parse(&bytes).unwrap();
    let digest = Sha256::of(&bytes);
    let (retained, owned) = (receipt(&contract, &digest, Role::Retained), receipt(&contract, &digest, Role::Composed));
    compare_with(&contract, &digest, retained, owned, |_| {}, |_, owned| edit(owned)).0
}

#[test]
fn equal_receipts_compare_equal() {
    assert_eq!(verdict(&fixture(), |_| {}), None);
}

#[test]
fn owned_only_boundaries_do_not_differ() {
    assert_eq!(verdict(&fixture(), |owned| {
        owned.rows.insert(0, row("first-stage/entry", [9, 9, 9, 9], 0x1000, vec![]));
    }), None);
}

#[test]
fn declarations_must_match_the_contract_and_be_admitted_by_the_boundary() {
    assert_eq!(verdict(&fixture(), |owned| owned.guard.as_mut().unwrap().declared[1].admitted = None),
        Some(Difference::DeclarationUnused { role: Role::Composed, index: 1 }));
    assert_eq!(verdict(&fixture(), |owned| { owned.guard.as_mut().unwrap().declared.pop(); }),
        Some(Difference::DeclarationList { role: Role::Composed }));
    let mut value = fixture();
    value["guard"]["declared"]["composed"][1]["writer"] = json!(24672);
    let bytes = serde_json::to_vec(&value).unwrap();
    let contract = Contract::parse(&bytes).unwrap();
    let digest = Sha256::of(&bytes);
    let retained = receipt(&contract, &digest, Role::Retained);
    let mut owned = receipt(&contract, &digest, Role::Composed);
    owned.guard.as_mut().unwrap().declared = receipt(&parse(&fixture()).unwrap(), &digest, Role::Composed).guard.unwrap().declared;
    assert_eq!(compare_with(&contract, &digest, retained, owned, |_| {}, |_, _| {}).0,
        Some(Difference::DeclarationList { role: Role::Composed }));
}

#[test]
fn an_owned_code_write_into_a_carrier_before_the_boundary_differs() {
    let write = write_row("cpu-write", 24580, Some(0), Some(24600));
    assert!(matches!(verdict(&fixture(), |owned| {
        let contract = parse(&fixture()).unwrap();
        owned.guard.as_mut().unwrap().carrier_writes_before_boundary = carriers(&contract, &[write]);
    }), Some(Difference::OwnedCodeWrite { start: 24576, end: 25772, .. })));
}

#[test]
fn a_missing_sequence_occurrence_is_the_first_difference() {
    assert_eq!(verdict(&fixture(), |owned| { owned.rows.pop(); owned.counts.pop(); }), Some(Difference::Sequence {
        core: Core::Primary, index: 2, retained: Some(label("primary-publisher/return-core0")), owned: None,
    }));
}

fn publisher_limit(limit: u32) -> Value {
    let mut value = fixture();
    for index in 0..2 {
        value["pieces"][1]["catalog"]["boundaries"][index]["record_limit"] = json!(limit);
    }
    value
}

fn interleave(receipt: &mut Receipt, order: [usize; 4], recorded: usize) {
    let (entry, ret) = (receipt.rows[1].clone(), receipt.rows[2].clone());
    receipt.rows.truncate(1);
    receipt.rows.extend(order.iter().take(recorded).map(|&which| if which == 0 { entry.clone() } else { ret.clone() }));
    receipt.counts[1].total = 2;
    receipt.counts[2].total = 2;
}

#[test]
fn order_past_record_limit_is_not_comparable() {
    let run = |value: &Value, recorded: usize| {
        let bytes = serde_json::to_vec(value).unwrap();
        let contract = Contract::parse(&bytes).unwrap();
        let digest = Sha256::of(&bytes);
        let mut retained = receipt(&contract, &digest, Role::Retained);
        let mut owned = receipt(&contract, &digest, Role::Composed);
        interleave(&mut retained, [0, 1, 0, 1], recorded);
        interleave(&mut owned, [0, 1, 1, 0], recorded);
        compare_with(&contract, &digest, retained, owned, |_| {}, |_, _| {}).0
    };
    assert_eq!(run(&publisher_limit(1), 2), Some(Difference::Unrecorded {
        label: label("primary-publisher/entry-core0"), role: Role::Retained, recorded: 1, total: 2,
    }));
    assert_eq!(run(&publisher_limit(2), 4), Some(Difference::Sequence {
        core: Core::Primary, index: 3,
        retained: Some(label("primary-publisher/entry-core0")), owned: Some(label("primary-publisher/return-core0")),
    }));
}

#[test]
fn a_hit_on_an_undeclared_core_differs() {
    assert_eq!(verdict(&fixture(), |owned| owned.off_core.push(LabelCount {
        label: label("primary-publisher/entry-core0"), core: Core::Secondary, total: 1,
    })), Some(Difference::OffCore { label: label("primary-publisher/entry-core0"), core: Core::Secondary, role: Role::Composed }));
}

#[test]
fn compared_registers_differ_by_boundary_kind() {
    assert_eq!(verdict(&fixture(), |owned| owned.rows[1].r1 = Word(0x77)), Some(Difference::Register {
        label: label("primary-publisher/entry-core0"), core: Core::Primary, occurrence: 0, register: Register::R1,
        retained: Word(0x11), owned: Word(0x77),
    }));
    assert_eq!(verdict(&fixture(), |owned| owned.rows[2].sp = Word(0x63424)), Some(Difference::Register {
        label: label("primary-publisher/return-core0"), core: Core::Primary, occurrence: 0, register: Register::Sp,
        retained: Word(0x63420), owned: Word(0x63424),
    }));
    assert_eq!(verdict(&fixture(), |owned| { owned.rows[2].r1 = Word(0); owned.rows[0].r0 = Word(0); }), None);
}

#[test]
fn a_guard_fault_or_unended_guard_differs() {
    assert_eq!(verdict(&fixture(), |owned| {
        let guard = owned.guard.as_mut().unwrap();
        guard.first_fault = Some("Write".into());
    }), Some(Difference::Guard { role: Role::Composed }));
    assert_eq!(verdict(&fixture(), |owned| owned.guard = None), Some(Difference::Guard { role: Role::Composed }));
}

fn write_row(source: &str, physical: u64, core: Option<u8>, pc: Option<u32>) -> Value {
    json!({"source": source, "physical": physical, "address": physical | 0x40000000, "size": 4, "core": core, "pc": pc,
        "instructions": 6_000_000, "tick": null, "value": 0, "before": [0, 0, 0, 0], "byte_order": "little-endian"})
}

fn stock_writes() -> Vec<Value> {
    vec![
        write_row("host-write", 24600, None, None),
        write_row("cpu-write", 263300, Some(0), Some(0x6c0010)),
        write_row("cpu-write", 263304, Some(0), Some(0x6c0010)),
    ]
}

fn rows(rows: &[Value]) -> Vec<WatchRow> { serde_json::from_value(Value::Array(rows.to_vec())).unwrap() }

fn carriers(contract: &Contract, writes: &[Value]) -> Vec<CarrierWrites> {
    let classes: Vec<(u64, u64)> = contract.watch_classes().iter().map(|class| (class.start, class.end)).collect();
    carrier_writes(&classes, &contract.owned_code_spellings(), &rows(writes)).unwrap()
}

fn range(start: u64, end: u64) -> ByteRange { ByteRange { start, end } }

#[test]
fn carrier_writes_record_counts_writers_coverage_and_owned_code_writers() {
    let contract = parse(&fixture()).unwrap();
    let summaries = carriers(&contract, &stock_writes());
    let cpu = Writer { kind: WriteKind::CpuWrite, core: Some(Core::Primary), pc: Some(0x6c0010) };
    let host = Writer { kind: WriteKind::HostWrite, core: None, pc: None };
    assert_eq!((summaries[0].start, summaries[0].end, summaries[0].total), (24576, 25772, 1));
    assert_eq!(summaries[0].writers, [WriterCount { writer: host, count: 1 }]);
    assert_eq!((summaries[0].coverage.clone(), summaries[0].kinds.clone()), (vec![range(24600, 24604)], vec![WriteKind::HostWrite]));
    assert_eq!((summaries[1].start, summaries[1].total), (263216, 2));
    assert_eq!(summaries[1].writers, [WriterCount { writer: cpu, count: 2 }]);
    assert_eq!(summaries[1].coverage, [range(263300, 263308)]);
    assert_eq!(summaries[1].first.iter().map(|row| (row.writer, row.physical, row.address)).collect::<Vec<_>>(),
        [(cpu, 263300, 0x40000000 | 263300), (cpu, 263304, 0x40000000 | 263304)]);
    assert!(summaries.iter().all(|summary| summary.owned_code_writes == 0));
    let many: Vec<Value> = (0..1025u32).map(|index| write_row("cpu-write", 263300, Some(1), Some(0x700000 + 4 * index))).collect();
    let truncated = &carriers(&contract, &many)[1];
    assert_eq!((truncated.total, truncated.writers.len(), truncated.writers_truncated, truncated.first.len()), (1025, 1024, true, 16));
    let owned = carriers(&contract, &[write_row("cpu-write", 263400, Some(0), Some(263220)), write_row("cpu-write", 263404, Some(0), Some(ALIAS as u32 + 263220))]);
    assert_eq!((owned[1].owned_code_writes, owned[1].first_owned_code_writes.len()), (2, 2));
}

#[test]
fn carrier_write_rows_count_in_every_class_they_overlap() {
    let mut before = write_row("host-write", 24570, None, None);
    before["size"] = json!(16);
    let summaries = carrier_writes(&[(24576, 25772)], &[], &rows(&[before])).unwrap();
    assert_eq!((summaries[0].total, summaries[0].coverage.clone()), (1, vec![range(24576, 24586)]));
    let mut spanning = write_row("model-bus-write", 0x10c, None, None);
    spanning["size"] = json!(16);
    let summaries = carrier_writes(&[(0x100, 0x110), (0x118, 0x120)], &[], &rows(&[spanning])).unwrap();
    assert_eq!(summaries.iter().map(|summary| (summary.total, summary.coverage.clone())).collect::<Vec<_>>(),
        [(1, vec![range(0x10c, 0x110)]), (1, vec![range(0x118, 0x11c)])]);
    assert!(carrier_writes(&[(24576, 25772)], &[], &rows(&[write_row("cpu-write", 0x100000, Some(0), Some(1))])).is_err());
}

#[test]
fn carrier_writes_compare_coverage_kinds_and_owned_writers_not_counts() {
    let contract = parse(&fixture()).unwrap();
    let set = |writes: Vec<Value>| {
        let summaries = carriers(&contract, &writes);
        move |owned: &mut Receipt| owned.guard.as_mut().unwrap().carrier_writes = summaries
    };
    let mut rewritten = stock_writes();
    rewritten.push(write_row("cpu-write", 263300, Some(1), Some(0x6c0099)));
    assert_eq!(verdict(&fixture(), set(rewritten)), None);
    let mut wider = stock_writes();
    wider.push(write_row("cpu-write", 263400, Some(0), Some(0x6c0010)));
    assert_eq!(verdict(&fixture(), set(wider)), Some(Difference::CarrierWrites { start: 263216, end: 264620, field: CarrierField::Coverage }));
    let mut kinds = stock_writes();
    kinds[0] = write_row("model-bus-write", 24600, None, None);
    assert_eq!(verdict(&fixture(), set(kinds)), Some(Difference::CarrierWrites { start: 24576, end: 25772, field: CarrierField::Kinds }));
    let mut owned_code = stock_writes();
    owned_code[2] = write_row("cpu-write", 263304, Some(0), Some(263220));
    assert!(matches!(verdict(&fixture(), set(owned_code)),
        Some(Difference::OwnedCodeWrite { start: 263216, end: 264620, first: Some(write) }) if write.writer.pc == Some(263220)));
}

#[test]
fn a_missing_menu_differs() {
    assert_eq!(verdict(&fixture(), |owned| owned.menu.shooting = false), Some(Difference::Menu { role: Role::Composed }));
    assert_eq!(verdict(&fixture(), |owned| owned.outcome = Outcome::Refused), Some(Difference::Menu { role: Role::Composed }));
}

#[test]
fn a_presence_boundary_absent_from_one_role_differs() {
    let mut value = fixture();
    value["pieces"][1]["catalog"]["boundaries"][1]["compare"] = json!("presence");
    assert_eq!(verdict(&value, |_| {}), None);
    assert_eq!(verdict(&value, |owned| { owned.rows.pop(); owned.counts.pop(); }),
        Some(Difference::Presence { label: label("primary-publisher/return-core0"), role: Role::Composed }));
}

#[test]
fn identities_and_reads_differ_unless_declared() {
    assert_eq!(verdict(&fixture(), |owned| owned.identity.update = digest('d')),
        Some(Difference::Identity { role: Role::Composed, field: IdentityField::Update }));
    assert_eq!(verdict(&fixture(), |owned| owned.identity.run = digest('d')),
        Some(Difference::Identity { role: Role::Composed, field: IdentityField::Run }));
    assert_eq!(verdict(&fixture(), |owned| owned.role = Role::Retained),
        Some(Difference::Identity { role: Role::Composed, field: IdentityField::Role }));
    let change = |owned: &mut Receipt| owned.rows[2].reads[0].bytes = HexBytes(vec![8; 4]);
    assert_eq!(verdict(&fixture(), change), Some(Difference::Read { label: label("primary-publisher/table-a"), occurrence: 0 }));
    let mut value = fixture();
    value["pieces"][1]["catalog"]["reads"][0]["differs"] = json!("owned publisher emits its own table");
    assert_eq!(verdict(&value, change), None);
}

#[test]
fn host_plan_lists_each_roles_armed_sites_guard_and_declarations() {
    use fujimod_xm1::composed_catalog::{ArmedSite, SourcePlan};
    let mut value = fixture();
    value["pieces"][1]["catalog"]["boundaries"][1]["cores"] = json!([0, 1]);
    let plan = parse(&value).unwrap().plan();
    let [retained, composed] = &plan.roles[..] else { panic!("plan has two roles") };
    assert_eq!((retained.role, composed.role), (Role::Retained, Role::Composed));
    let site = |pc, text: &str, stop| ArmedSite { pc: Pc(pc), label: label(text), stop };
    let sites = |role: &fujimod_xm1::composed_catalog::RolePlan, core: Core| role.armed.iter()
        .find(|armed| armed.core == core).unwrap().sites.clone();
    assert_eq!(sites(composed, Core::Primary), [
        site(24576, "first-stage/entry", false), site(24580, "first-stage/handoff", true),
        site(263216, "primary-publisher/entry-core0", true), site(264000, "primary-publisher/return-core0", true),
    ]);
    assert_eq!(sites(composed, Core::Secondary), [site(264000, "primary-publisher/return-core0", true)]);
    assert_eq!(sites(retained, Core::Primary).len(), 3);
    assert_eq!(composed.stop_labels.len(), 3);
    assert_eq!(composed.watch_classes, [PlanSpan { start: 24576, end: 25772 }, PlanSpan { start: 263216, end: 264620 }]);
    assert_eq!(composed.policies.len(), 4);
    assert!(retained.policies.is_empty());
    assert_eq!(composed.declared.len(), 2);
    assert_eq!(retained.declared.len(), 2);
    let cpu1 = composed.guard.iter().find(|policy| policy.source == SourcePlan::Cpu { core: 1 }).unwrap();
    assert!(cpu1.allow.contains(&RangePlan::Tcm { core: 1, bank: Bank::Instruction, start: 0, end: END }));
    let partition = &cpu1.partitions[1];
    assert_eq!(partition.spans, spellings(263216, 263924));
    assert_eq!(partition.allow, [physical_plan(318176, 323852), physical_plan(404564, 406624)]);
    assert_eq!(composed.guard.len(), 6);
    assert!(retained.guard.iter().all(|policy| policy.partitions.is_empty()));
    assert!(serde_json::to_value(&plan).is_ok());
}

fn attach_rules(value: &mut Value) -> &mut Value { &mut value["guard"]["attach_state"] }

fn allowance_label(text: &str) -> AllowanceLabel { AllowanceLabel::try_from(text.to_owned()).unwrap() }

fn declared(label: &str, start: u64, end: u64) -> Value {
    json!({"label": label, "kind": "declared", "space": {"target": "physical"}, "start": start, "end": end, "reason": "r", "citation": "c"})
}

fn physical_window(start: u64, end: u64) -> Value { json!({"space": {"target": "physical"}, "start": start, "end": end, "citation": "c"}) }

fn with_allowances(allowances: Value) -> Value {
    let mut value = fixture();
    attach_rules(&mut value)["allowances"] = allowances;
    value
}

fn attach_refused(edit: impl FnOnce(&mut Value)) -> AttachRuleRefusal {
    match refused(|value| edit(attach_rules(value))) {
        CatalogRefusal::AttachState(refusal) => refusal,
        other => panic!("expected an attach-state refusal, got {other:?}"),
    }
}

fn zeros(length: usize) -> Sha256 { Sha256::of(&vec![0; length]) }

#[test]
fn attach_rules_refuse_overlaps_sizes_masks_and_shapes() {
    assert_eq!(attach_refused(|rules| rules["allowances"] = json!([declared("a", 0x100, 0x200), declared("b", 0x1f0, 0x300)])),
        AttachRuleRefusal::Overlap { first: Occupant::Allowance(allowance_label("a")), second: Occupant::Allowance(allowance_label("b")) });
    assert_eq!(attach_refused(|rules| rules["allowances"] = json!([declared("a", 0, 0x10001)])),
        AttachRuleRefusal::DeclaredSize(allowance_label("a")));
    assert_eq!(attach_refused(|rules| rules["registers"] = json!([{"core": 0, "register": "r7", "mask": 0xff, "retained": 0, "composed": 1, "reason": "r"}])),
        AttachRuleRefusal::RegisterMask { core: Core::Primary, register: ContextRegister::R7 });
    assert_eq!(attach_refused(|rules| rules["registers"] = json!([{"core": 0, "register": "cpsr", "mask": 0x100, "retained": 0, "composed": 0x200, "reason": "r"}])),
        AttachRuleRefusal::RegisterMask { core: Core::Primary, register: ContextRegister::Cpsr });
    assert_eq!(attach_refused(|rules| rules["registers"] = json!([{"core": 0, "register": "cpsr", "mask": 0x100,
        "retained": {"row": "first-stage/handoff"}, "composed": 0x100, "reason": "r"}])),
        AttachRuleRefusal::RegisterRow { core: Core::Primary, register: ContextRegister::Cpsr });
    assert_eq!(attach_refused(|rules| rules["peers"] = json!([])), AttachRuleRefusal::Peers);
    assert_eq!(attach_refused(|rules| rules["carriers"][0]["parts"][0]["end"] = json!(25000)),
        AttachRuleRefusal::CarrierTiling { piece: first_stage() });
    assert_eq!(attach_refused(|rules| rules["carriers"][0]["parts"][0]["reason"] = Value::Null),
        AttachRuleRefusal::CarrierPartShape { piece: first_stage() });
    assert_eq!(attach_refused(|rules| {
        rules["carriers"][0]["parts"][0] = json!({"start": 24576, "end": 25772, "residency": "resident", "reason": null,
            "canonical_sha256": zeros(1196), "composed_sha256": zeros(1196)});
        rules["allowances"] = json!([declared("over-carrier", 25000, 25004)]);
    }), AttachRuleRefusal::Overlap { first: Occupant::Carrier(first_stage()), second: Occupant::Allowance(allowance_label("over-carrier")) });
    assert_eq!(attach_refused(|rules| rules["allowances"] = json!([{
        "label": "live", "kind": "peer-live-stack", "space": {"target": "physical"}, "start": 0x70000, "end": 0x70100, "reason": "r",
        "core": 0, "window": physical_window(0x6f000, 0x71000)}])), AttachRuleRefusal::PeerAttachCore(allowance_label("live")));
    assert_eq!(attach_refused(|rules| rules["allowances"] = json!([{
        "label": "stack", "kind": "dead-stack", "space": {"target": "physical"}, "start": 0x63000, "end": 0x63400, "reason": "r",
        "core": 0, "mode": "svc", "window": physical_window(0x63100, 0x64000)}])), AttachRuleRefusal::OutsideWindow(allowance_label("stack")));
    assert_eq!(attach_refused(|rules| rules["allowances"] = json!([{
        "label": "stack", "kind": "declared", "space": {"target": "physical"}, "start": 0x63000, "end": 0x63400, "reason": "r",
        "citation": "c", "core": 0}])), AttachRuleRefusal::AllowanceShape(allowance_label("stack")));
    for pointer in ["/bus", "/bus/devices", "/bus/devices/0", "/bus/devices/0/state", "/bus/frontier/0", "/bus/unmodeled/0",
        "/gic", "/timeline/now", "/quantum/x", "/unknown/x"] {
        assert!(matches!(refused(|value| attach_rules(value)["board"] = json!([{"pointer": pointer, "kind": "declared", "reason": "r", "citation": "c"}])),
            CatalogRefusal::Shape(_)), "{pointer}");
    }
    for pointer in ["/bus/devices/0/state/count", "/bus/devices/0/kind", "/bus/frontier/0/tick", "/bus/mode"] {
        let mut value = fixture();
        attach_rules(&mut value)["board"] = json!([{"pointer": pointer, "kind": "declared", "reason": "r", "citation": "c"}]);
        assert!(parse(&value).is_ok(), "{pointer}");
    }
    assert_eq!(attach_refused(|rules| rules["peers"][0]["retained_pc"] = json!([{"start": 0xd1000, "end": 0xd2000}, {"start": 0xd0000, "end": 0xd1800}])),
        AttachRuleRefusal::PeerSpans(Core::Secondary));
    assert!(matches!(attach_refused(|rules| rules["board"] = json!([
        {"pointer": "/gic/pending", "kind": "declared", "reason": "r", "citation": "c"},
        {"pointer": "/gic/pending/0", "kind": "declared", "reason": "r", "citation": "c"}])), AttachRuleRefusal::BoardOverlap(_)));
}

#[test]
fn an_undeclared_memory_difference_refuses_with_its_runs() {
    let (difference, _) = attach_verdict(&fixture(), |owned| { owned.bytes.insert((Space::Physical, 0x300), 1); owned.bytes.insert((Space::Physical, 0x301), 2); });
    assert_eq!(difference, Some(Difference::AttachMemory {
        space: Space::Physical, start: 0x300, end: 0x302, differing_bytes: 2,
        runs: vec![SpaceSpan { space: Space::Physical, start: 0x300, end: 0x302 }],
    }));
    let (difference, _) = attach_verdict(&fixture(), |owned| { owned.bytes.insert((Space::Tcm { core: Core::Primary, bank: Bank::Instruction }, 0x1ff8), 7); });
    assert!(matches!(difference, Some(Difference::AttachMemory { space: Space::Tcm { core: Core::Primary, bank: Bank::Instruction }, start: 0x1ff8, .. })));
}

#[test]
fn a_declared_allowance_absorbs_and_records_its_bytes_and_an_unused_one_refuses() {
    let value = with_allowances(json!([declared("timer-word", 0x300, 0x304)]));
    let (difference, attach) = attach_verdict(&value, |owned| { owned.bytes.insert((Space::Physical, 0x301), 9); });
    assert_eq!(difference, None);
    let absorbed = &attach.unwrap().allowances[0];
    assert_eq!((absorbed.differing_bytes, absorbed.ranges.clone(), absorbed.retained.clone(), absorbed.composed.clone()),
        (1, vec![Span::new(0x301, 0x302).unwrap()], vec!["00".to_owned()], vec!["09".to_owned()]));
    assert_eq!(attach_verdict(&value, |_| {}).0, Some(Difference::UnusedAllowance { entry: UnusedEntry::Memory(allowance_label("timer-word")) }));
}

#[test]
fn resident_carriers_and_copies_must_hash_to_each_roles_image() {
    let mut value = fixture();
    attach_rules(&mut value)["carriers"][1]["parts"][0] = json!({"start": 263216, "end": 264620, "residency": "resident", "reason": null,
        "canonical_sha256": zeros(1404), "composed_sha256": Sha256::of(&[vec![0; 4], vec![0xaa], vec![0; 1399]].concat())});
    assert_eq!(attach_verdict(&value, |owned| { owned.bytes.insert((Space::Physical, 263220), 0xaa); }).0, None);
    assert_eq!(attach_verdict(&value, |_| {}).0, Some(Difference::AttachCarrier {
        piece: publisher(), target: Target::LoaderA, start: 263216, end: 264620, role: Role::Composed,
    }));
    let copy = |composed: Sha256| with_allowances(json!([{
        "label": "staged-record", "kind": "copy", "space": {"target": "physical"}, "start": 0x50000, "end": 0x50010, "reason": "r",
        "source": {"dat": {"offset": 64, "transform": "complement"}}, "canonical_sha256": zeros(16), "composed_sha256": composed}]));
    assert_eq!(attach_verdict(&copy(zeros(16)), |_| {}).0, None);
    assert_eq!(attach_verdict(&copy(digest('e')), |_| {}).0,
        Some(Difference::AttachCopy { label: allowance_label("staged-record"), role: Role::Composed }));
}

#[test]
fn stack_allowances_are_checked_against_the_captured_stack_pointers() {
    let dead = |start: u64, end: u64, window: Value| with_allowances(json!([{
        "label": "dead", "kind": "dead-stack", "space": {"target": "physical"}, "start": start, "end": end, "reason": "r",
        "core": 0, "mode": "svc", "window": window}]));
    let below_sp = |owned: &mut Spec| { owned.bytes.insert((Space::Physical, 0x63010), 1); };
    assert_eq!(attach_verdict(&dead(0x63000, 0x63400, physical_window(0x63000, 0x64000)), below_sp).0, None);
    let failed = |check| Some(Difference::AttachAllowance { label: allowance_label("dead"), role: Role::Retained, check });
    assert_eq!(attach_verdict(&dead(0x63400, 0x63460, physical_window(0x63000, 0x64000)), below_sp).0, failed(StackCheck::AboveSp));
    assert_eq!(attach_verdict(&dead(0x60000, 0x60100, physical_window(0x60000, 0x61000)), below_sp).0, failed(StackCheck::SpOutsideWindow));
    let mut irq = dead(0x63000, 0x63400, physical_window(0x63000, 0x64000));
    attach_rules(&mut irq)["allowances"][0]["mode"] = json!("irq");
    assert_eq!(attach_verdict(&irq, below_sp).0, failed(StackCheck::Mode));

    let abandoned = |window: Value| with_allowances(json!([{
        "label": "dead", "kind": "dead-stack-at-boundary", "space": {"target": "physical"}, "start": 0x63000, "end": 0x63100,
        "reason": "r", "core": 0, "boundary": "first-stage/handoff", "window": window}]));
    assert_eq!(attach_verdict(&abandoned(physical_window(0x63000, 0x63410)), below_sp).0, None);
    assert_eq!(attach_verdict(&abandoned(physical_window(0x62000, 0x64000)), below_sp).0, failed(StackCheck::StillCurrent));

    let live = |start: u64| with_allowances(json!([{
        "label": "dead", "kind": "peer-live-stack", "space": {"target": "physical"}, "start": start, "end": 0x70100, "reason": "r",
        "core": 1, "window": physical_window(0x6f000, 0x71000)}]));
    let above_peer_sp = |owned: &mut Spec| { owned.bytes.insert((Space::Physical, 0x70020), 1); };
    assert_eq!(attach_verdict(&live(0x70000), above_peer_sp).0, None);
    assert_eq!(attach_verdict(&live(0x6ff00), above_peer_sp).0, failed(StackCheck::BelowPeerSp));
}

#[test]
fn context_differences_need_an_exact_masked_register_entry() {
    let set_a = |owned: &mut Spec| owned.cores[0].cpsr = Word(0x6000_01d3);
    assert_eq!(attach_verdict(&fixture(), set_a).0, Some(Difference::AttachContext {
        core: Core::Primary, register: ContextRegister::Cpsr, retained: Word(0x6000_00d3), owned: Word(0x6000_01d3),
    }));
    let mut value = fixture();
    attach_rules(&mut value)["registers"] = json!([{"core": 0, "register": "cpsr", "mask": 0x100, "retained": 0, "composed": 0x100, "reason": "r"}]);
    let (difference, attach) = attach_verdict(&value, set_a);
    assert_eq!(difference, None);
    assert_eq!(attach.unwrap().registers.len(), 1);
    assert!(matches!(attach_verdict(&value, |owned| owned.cores[0].cpsr = Word(0x6000_01d2)).0, Some(Difference::AttachContext { .. })));
    assert_eq!(attach_verdict(&value, |_| {}).0,
        Some(Difference::UnusedAllowance { entry: UnusedEntry::Register { core: Core::Primary, register: ContextRegister::Cpsr } }));
    assert!(matches!(attach_verdict(&fixture(), |owned| owned.cores[0].r[7] = Word(0x7000_0000)).0,
        Some(Difference::AttachContext { register: ContextRegister::R7, .. })));
    assert_eq!(attach_verdict(&fixture(), |owned| owned.cores[1].pc = Word(0x20000)).0,
        Some(Difference::AttachPeer { core: Core::Secondary, role: Role::Composed, pc: Word(0x20000) }));
    assert_eq!(attach_verdict(&fixture(), |owned| owned.cores[1].r[3] = Word(9)).0, None);
}

#[test]
fn a_row_valued_register_expectation_reads_that_roles_last_row_before_attach() {
    let run = |row: &str, register: u32, instructions: [u64; 2]| {
        let mut value = fixture();
        attach_rules(&mut value)["registers"] = json!([{"core": 0, "register": "r5", "mask": 0xffff_ffffu32,
            "retained": 0, "composed": {"row": row}, "reason": "r"}]);
        let bytes = serde_json::to_vec(&value).unwrap();
        let contract = Contract::parse(&bytes).unwrap();
        let digest = Sha256::of(&bytes);
        let (retained, owned) = (receipt(&contract, &digest, Role::Retained), receipt(&contract, &digest, Role::Composed));
        full_compare_with(&contract, &digest, retained, owned, |owned| owned.cores[0].r[5] = Word(register),
            |_, owned| owned.attach.as_mut().unwrap().instructions = instructions)
    };
    let attach = [5_000_000, 886_999];
    let result = run("first-stage/handoff", 5, attach);
    assert_eq!(result.first_difference, None);
    assert_eq!(result.attach.unwrap().registers.len(), 1);
    assert!(matches!(run("first-stage/handoff", 9, attach).first_difference,
        Some(Difference::AttachContext { register: ContextRegister::R5, .. })));
    let row_refused = |verdict: Verdict| verdict.attach_differences.entries.iter()
        .any(|entry| matches!(entry, Difference::AttachRegisterRow { register: ContextRegister::R5, role: Role::Composed, .. }));
    assert!(row_refused(run("first-stage/entry", 5, attach)));
    assert_eq!(run("first-stage/handoff", 5, [600, 600]).first_difference, None);
    assert!(row_refused(run("first-stage/handoff", 5, [500, 400])));
}

#[test]
fn a_row_valued_register_expectation_takes_the_last_of_several_rows_in_either_role() {
    let mut value = publisher_limit(2);
    attach_rules(&mut value)["registers"] = json!([{"core": 0, "register": "r5", "mask": 0xffff_ffffu32,
        "retained": {"row": "primary-publisher/entry-core0"}, "composed": 0x50, "reason": "r"}]);
    let bytes = serde_json::to_vec(&value).unwrap();
    let contract = Contract::parse(&bytes).unwrap();
    let digest = Sha256::of(&bytes);
    let run = |attach_r5: u32| {
        let (mut retained, mut owned) = (receipt(&contract, &digest, Role::Retained), receipt(&contract, &digest, Role::Composed));
        interleave(&mut retained, [0, 1, 0, 1], 4);
        interleave(&mut owned, [0, 1, 0, 1], 4);
        retained.rows[1].r5 = Word(0x70);
        retained.rows[3].r5 = Word(0x80);
        let (mut retained_spec, mut owned_spec) = (spec(Role::Retained), spec(Role::Composed));
        retained_spec.cores[0].r[5] = Word(attach_r5);
        owned_spec.cores[0].r[5] = Word(0x50);
        compare_specs(&contract, &digest, retained, owned, &retained_spec, &owned_spec, |_, _| {})
    };
    assert_eq!(run(0x80).first_difference, None);
    assert!(matches!(run(0x70).first_difference, Some(Difference::AttachContext { register: ContextRegister::R5, .. })));
}

#[test]
fn board_differences_need_an_exact_or_declared_entry() {
    let set_count = |owned: &mut Spec| owned.board["bus"]["devices"][0]["state"]["count"] = json!(2);
    assert_eq!(attach_verdict(&fixture(), set_count).0, Some(Difference::AttachBoard {
        pointer: BoardLeaf("/bus/devices/0/state/count".into()), retained: "1".into(), owned: "2".into(),
    }));
    assert_eq!(attach_verdict(&fixture(), |owned| owned.board["timeline"]["now"] = json!(9)).0, None);
    let entry = |kind: Value| {
        let mut value = fixture();
        attach_rules(&mut value)["board"] = json!([kind]);
        value
    };
    let exact = entry(json!({"pointer": "/bus/devices/0/state/count", "kind": "exact", "retained": 1, "composed": 2, "reason": "r"}));
    let (difference, attach) = attach_verdict(&exact, set_count);
    assert_eq!(difference, None);
    assert_eq!(attach.unwrap().board[0].leaves, [BoardLeaf("/bus/devices/0/state/count".into())]);
    assert!(matches!(attach_verdict(&exact, |owned| owned.board["bus"]["devices"][0]["state"]["count"] = json!(3)).0, Some(Difference::AttachBoard { .. })));
    let declared_entry = entry(json!({"pointer": "/bus/devices/0/state/count", "kind": "declared", "reason": "r", "citation": "c"}));
    assert_eq!(attach_verdict(&declared_entry, |owned| owned.board["bus"]["devices"][0]["state"]["count"] = json!(3)).0, None);
    assert!(matches!(attach_verdict(&declared_entry, |owned| owned.board["bus"]["devices"][0]["state"]["mode"] = json!(1)).0,
        Some(Difference::AttachBoard { .. })));
    let exact_null = entry(json!({"pointer": "/memory_decoders/pending/0", "kind": "exact", "retained": null, "composed": 3, "reason": "r"}));
    assert_eq!(attach_verdict(&exact_null, |owned| owned.board["memory_decoders"]["pending"][0] = json!(3)).0, None);
    assert!(matches!(attach_verdict(&fixture(), |owned| { owned.board.as_object_mut().unwrap().insert("added".into(), json!(1)); }).0,
        Some(Difference::AttachSnapshot { role: Role::Composed, .. })));
}

#[test]
fn a_board_refusal_does_not_hide_memory_differences() {
    let tcm = Space::Tcm { core: Core::Primary, bank: Bank::Instruction };
    let result = attach_report(&fixture(), |owned| {
        owned.board["bus"]["devices"][0]["state"]["count"] = json!(2);
        owned.bytes.insert((Space::Physical, 0x300), 1);
        owned.bytes.insert((tcm, 0x1ff8), 7);
    });
    let board = Difference::AttachBoard { pointer: BoardLeaf("/bus/devices/0/state/count".into()), retained: "1".into(), owned: "2".into() };
    assert_eq!(result.verdict, VerdictKind::Different);
    assert_eq!(result.first_difference, Some(board.clone()));
    assert_eq!(result.attach_differences.entries, [
        board,
        Difference::AttachMemory {
            space: Space::Physical, start: 0x300, end: 0x301, differing_bytes: 1,
            runs: vec![SpaceSpan { space: Space::Physical, start: 0x300, end: 0x301 }],
        },
        Difference::AttachMemory { space: tcm, start: 0x1ff8, end: 0x1ff9, differing_bytes: 1, runs: vec![SpaceSpan { space: tcm, start: 0x1ff8, end: 0x1ff9 }] },
    ]);
    assert_eq!((result.attach_differences.total, result.attach_differences.truncated), (3, false));
}

#[test]
fn attach_differences_stop_at_the_limit_and_keep_the_total() {
    let result = attach_report(&fixture(), |owned| {
        let state = owned.board["bus"]["devices"][0]["state"].as_object_mut().unwrap();
        for index in 0..300 {
            state.insert(format!("added-{index:03}"), json!(index));
        }
    });
    let differences = &result.attach_differences;
    assert_eq!((differences.entries.len(), differences.total, differences.truncated), (attach_state::ATTACH_DIFFERENCE_LIMIT, 300, true));
    assert_eq!(result.first_difference, Some(Difference::AttachBoard {
        pointer: BoardLeaf("/bus/devices/0/state/added-000".into()), retained: "absent".into(), owned: "0".into(),
    }));
    assert_eq!(differences.entries.last(), Some(&Difference::AttachBoard {
        pointer: BoardLeaf("/bus/devices/0/state/added-255".into()), retained: "absent".into(), owned: "255".into(),
    }));
}

#[test]
fn a_difference_outside_the_attach_state_lists_no_attach_differences() {
    let bytes = serde_json::to_vec(&fixture()).unwrap();
    let contract = Contract::parse(&bytes).unwrap();
    let digest = Sha256::of(&bytes);
    let (retained, owned) = (receipt(&contract, &digest, Role::Retained), receipt(&contract, &digest, Role::Composed));
    let result = full_compare_with(&contract, &digest, retained, owned, |_| {}, |_, owned| { owned.rows.pop(); owned.counts.pop(); });
    assert!(matches!(result.first_difference, Some(Difference::Sequence { .. })));
    assert_eq!(result.attach_differences.total, 0);
}

#[test]
fn an_absent_carrier_part_compares_as_memory_and_admits_allowances() {
    let in_carrier = |owned: &mut Spec| { owned.bytes.insert((Space::Physical, 24600), 3); };
    assert!(matches!(attach_verdict(&fixture(), in_carrier).0, Some(Difference::AttachMemory { start: 24600, .. })));
    let (difference, attach) = attach_verdict(&with_allowances(json!([declared("carrier-word", 24600, 24604)])), in_carrier);
    assert_eq!(difference, None);
    assert_eq!(attach.unwrap().allowances[0].differing_bytes, 1);
}

#[test]
fn a_capture_file_that_differs_from_its_binding_refuses() {
    let bytes = serde_json::to_vec(&fixture()).unwrap();
    let contract = Contract::parse(&bytes).unwrap();
    let digest = Sha256::of(&bytes);
    let (retained, owned) = (receipt(&contract, &digest, Role::Retained), receipt(&contract, &digest, Role::Composed));
    let (difference, _) = compare_with(&contract, &digest, retained.clone(), owned.clone(), |_| {}, |directory, _| {
        let path = directory.join(attach_state::MEMORY_FILE);
        let mut file = std::fs::read(&path).unwrap();
        let last = file.len() - 1;
        file[last] ^= 1;
        std::fs::write(&path, file).unwrap();
    });
    assert!(matches!(difference, Some(Difference::AttachSnapshot { role: Role::Composed, .. })));
    let (difference, _) = compare_with(&contract, &digest, retained.clone(), owned.clone(), |_| {}, |_, owned| {
        owned.attach.as_mut().unwrap().regions[0].sha256 = zeros(4);
    });
    assert!(matches!(difference, Some(Difference::AttachSnapshot { role: Role::Composed, .. })));
    let (difference, _) = compare_with(&contract, &digest, retained.clone(), owned.clone(), |_| {}, |_, owned| {
        owned.attach.as_mut().unwrap().unreachable.clear();
    });
    assert!(matches!(difference, Some(Difference::AttachSnapshot { role: Role::Composed, .. })));
    let (difference, _) = compare_with(&contract, &digest, retained.clone(), owned.clone(), |_| {}, |_, owned| {
        owned.attach.as_mut().unwrap().regions[0].reads[0].path = ReadPath::Bus { address: BusAddress(0) };
    });
    assert!(matches!(difference, Some(Difference::AttachSnapshot { role: Role::Composed, .. })));
    let (difference, _) = compare_with(&contract, &digest, retained, owned, |_| {}, |_, owned| owned.attach = None);
    assert!(matches!(difference, Some(Difference::AttachSnapshot { role: Role::Composed, .. })));
}

#[test]
fn a_diagnostic_receipt_is_not_evidence() {
    assert_eq!(verdict(&fixture(), |owned| owned.kind = ReceiptKind::ComposedStartupDiagnostic),
        Some(Difference::Identity { role: Role::Composed, field: IdentityField::Kind }));
}

#[derive(Default)]
struct Clock {
    now: u64,
}

impl Host for Clock {
    fn now(&self) -> u64 { self.now }
    fn schedule(&mut self, _: usize, _: u64, _: u64) {}
    fn set_irq(&mut self, _: u32, _: bool) {}
    fn raise_irq(&mut self, _: usize, _: u32) -> Result<(), String> { Ok(()) }
    fn ram_read(&self, _: u64, _: &mut [u8]) -> Result<(), String> { Err("ram".into()) }
    fn ram_write(&mut self, _: u64, _: &[u8]) -> Result<(), String> { Err("ram".into()) }
    fn record(&mut self, _: Record) {}
}

#[derive(Clone, Copy)]
struct Step {
    tick: u64,
    core: Core,
    pc: u32,
    offset: u32,
    write: Option<u32>,
}

const RETAINED_SITE: u32 = 0x40f4;
const COMPOSED_SITE: u32 = 0x64a8;
const POLL_PC: u32 = 0x3000;
const LOG: u64 = 0x400;

fn put(tick: u64, offset: u32, value: u32) -> Step { Step { tick, core: Core::Primary, pc: 0x2000, offset, write: Some(value) } }

fn get(tick: u64, pc: u32, offset: u32) -> Step { Step { tick, core: Core::Primary, pc, offset, write: None } }

fn on_secondary(step: Step) -> Step { Step { core: Core::Secondary, ..step } }

/// Counter start, then two log entries `gap` and `2 * gap` ticks after `base` with a polling read between.
fn timer_steps(base: u64, gap: u64, site: u32) -> Vec<Step> {
    vec![put(base, 0x00, 0), put(base + 1, 0x04, 0xffff_ffff), put(base + 2, 0x08, 0x63),
        get(base + gap, site, 0x00), get(base + gap + 10, POLL_PC, 0x00), get(base + 2 * gap, site, 0x00)]
}

/// Runs `steps` through FujiMod's timer from reset, then lets a due deadline settle at `settle`.
fn run_timer(steps: &[Step], settle: Option<u64>) -> (Vec<TransactionRow>, Value) {
    let inputs = || fujimod_emulator_devices::exr2::irq::Inputs::default();
    let mut model = Timers::new(Options { main_targets: vec![0, 1] }, Wiring { controllers: vec![(0, inputs()), (1, inputs())], start_tick: 0 }).unwrap();
    let mut clock = Clock::default();
    let rows = steps.iter().map(|step| {
        clock.now = step.tick;
        let mut cx = Cx::new(&mut clock, step.core.index(), u64::from(step.pc), 0);
        let address = timer::BASE + u64::from(step.offset);
        let value = match step.write {
            Some(value) => {
                model.write(&mut cx, address, 4, u64::from(value));
                value
            }
            None => model.read(&mut cx, address, 4) as u32,
        };
        assert_eq!(cx.refusal(), None);
        TransactionRow {
            tick: step.tick, core: Some(step.core), pc: Pc(step.pc), address: address as u32, size: 4,
            access: if step.write.is_some() { Access::Write } else { Access::Read }, accepted: true, value,
        }
    }).collect();
    if let Some(tick) = settle {
        clock.now = tick;
        model.advance(&mut Cx::new(&mut clock, usize::MAX, 0, 0));
    }
    (rows, model.save())
}

/// A role whose timer runs `steps`; each site read, on either core, is logged at the next slot with its value word.
fn relation_spec(role: Role, steps: &[Step], site: u32) -> Spec { relation_spec_settled(role, steps, site, None) }

fn relation_spec_settled(role: Role, steps: &[Step], site: u32, settle: Option<u64>) -> Spec {
    let (rows, timer_state) = run_timer(steps, settle);
    let mut spec = spec(role);
    let mut log_writes = Vec::new();
    for (k, row) in rows.iter().filter(|row| row.pc.0 == site).enumerate() {
        let slot = LOG + 8 * k as u64;
        let event = 0x0100_0300 + k as u32;
        let core = row.core.unwrap();
        log_writes.push(LogWrite { core, pc: Pc(site + 4), physical: slot, size: 4, value: Word(row.value), tick: row.tick });
        log_writes.push(LogWrite { core, pc: Pc(0x5000), physical: slot + 4, size: 4, value: Word(event), tick: row.tick + 1 });
        set_word(&mut spec, slot, row.value);
        set_word(&mut spec, slot + 4, event);
    }
    let last = settle.into_iter().chain(rows.iter().map(|row| row.tick)).max().unwrap_or(0);
    spec.board["timeline"]["now"] = json!(last + 10);
    spec.board["bus"]["devices"] = json!([{
        "kind": "exr2-system-timers", "windows": [{"base": timer::BASE, "size": 64}], "native_dependencies": [0, 1], "state": timer_state,
    }]);
    spec.sources = Some(SourceSpec { reset: SystemTimer::new(vec![0, 1]).unwrap(), rows, log_writes });
    spec
}

fn set_word(spec: &mut Spec, address: u64, word: u32) {
    for (index, byte) in word.to_le_bytes().into_iter().enumerate() {
        spec.bytes.insert((Space::Physical, address + index as u64), byte);
    }
}

fn with_relations(mut value: Value) -> Value {
    value["guard"]["attach_state"]["relations"] = json!({
        "timer": {"citation": "timer.rs"},
        "log": {"span": {"start": LOG, "end": LOG + 0x80}, "sites": [
            {"label": "stock-record1", "role": "retained", "core": 0, "read_pc": RETAINED_SITE, "store_pc": RETAINED_SITE + 4, "citation": "c"},
            {"label": "stock-record1-core1", "role": "retained", "core": 1, "read_pc": RETAINED_SITE, "store_pc": RETAINED_SITE + 4, "citation": "c"},
            {"label": "owned-first-stage", "role": "composed", "core": 0, "read_pc": COMPOSED_SITE, "store_pc": COMPOSED_SITE + 4, "citation": "c"},
            {"label": "owned-first-stage-core1", "role": "composed", "core": 1, "read_pc": COMPOSED_SITE, "store_pc": COMPOSED_SITE + 4, "citation": "c"},
        ]},
        "lz": null,
    });
    value
}

fn relation_verdict(value: &Value, retained: &Spec, owned: &Spec) -> Verdict {
    let bytes = serde_json::to_vec(value).unwrap();
    let contract = Contract::parse(&bytes).unwrap();
    let digest = Sha256::of(&bytes);
    let receipts = (receipt(&contract, &digest, Role::Retained), receipt(&contract, &digest, Role::Composed));
    compare_specs(&contract, &digest, receipts.0, receipts.1, retained, owned, |_, _| {})
}

fn relation_difference(retained_steps: &[Step], owned_steps: &[Step], edit: impl FnOnce(&mut Spec)) -> Option<Difference> {
    let retained = relation_spec(Role::Retained, retained_steps, RETAINED_SITE);
    let mut owned = relation_spec(Role::Composed, owned_steps, COMPOSED_SITE);
    edit(&mut owned);
    relation_verdict(&with_relations(fixture()), &retained, &owned).first_difference
}

fn composed_failure(difference: Option<Difference>) -> Option<RelationFailure> {
    match difference {
        Some(Difference::AttachRelation { failure, .. }) => Some(failure),
        _ => None,
    }
}

#[test]
fn relations_explain_the_pace_dependent_timer_leaves_and_timestamps() {
    let retained = relation_spec(Role::Retained, &timer_steps(100, 200, RETAINED_SITE), RETAINED_SITE);
    let owned = relation_spec(Role::Composed, &timer_steps(160, 300, COMPOSED_SITE), COMPOSED_SITE);
    assert_ne!(retained.board["bus"]["devices"][0]["state"]["count"], owned.board["bus"]["devices"][0]["state"]["count"]);
    assert_ne!(retained.bytes.get(&(Space::Physical, LOG)), owned.bytes.get(&(Space::Physical, LOG)));
    let verdict = relation_verdict(&with_relations(fixture()), &retained, &owned);
    assert_eq!(verdict.first_difference, None);
    let relations = verdict.attach.unwrap().relations;
    assert_eq!(relations[1].timestamps.iter().map(|word| word.address).collect::<Vec<_>>(), [LOG, LOG + 8]);
    let without = relation_verdict(&fixture(), &retained, &owned).first_difference;
    assert!(matches!(without, Some(Difference::AttachBoard { .. })), "{without:?}");
}

#[test]
fn the_roles_must_program_the_timer_alike() {
    let retained = timer_steps(100, 200, RETAINED_SITE);
    let mut restarted = timer_steps(160, 300, COMPOSED_SITE);
    restarted.insert(4, put(160 + 301, 0x00, 0));
    assert!(matches!(composed_failure(relation_difference(&retained, &restarted, |_| {})), Some(RelationFailure::TimerSequence { .. })));
    let delay = |base: u64, control: u32| [put(base + 3, 0x14, 0xffff_ffff), put(base + 4, 0x18, control), get(base + 5, POLL_PC, 0x10), put(base + 6, 0x18, 0)];
    let with_delay = |base, gap, site, control| {
        let mut steps = timer_steps(base, gap, site);
        steps.splice(3..3, delay(base, control));
        steps
    };
    assert_eq!(relation_difference(&with_delay(100, 200, RETAINED_SITE, 0x23), &with_delay(160, 300, COMPOSED_SITE, 0x23), |_| {}), None);
    let failure = composed_failure(relation_difference(&with_delay(100, 200, RETAINED_SITE, 0x23), &with_delay(160, 300, COMPOSED_SITE, 0x53), |_| {}));
    assert!(matches!(failure, Some(RelationFailure::TimerSequence { retained: Some(Token::Write { offset: 0x18, .. }), .. })), "{failure:?}");
}

#[test]
fn log_words_must_hold_their_own_role_reads() {
    let (retained, owned) = (timer_steps(100, 200, RETAINED_SITE), timer_steps(160, 300, COMPOSED_SITE));
    let stored = |owned: &mut Spec| {
        let sources = owned.sources.as_mut().unwrap();
        sources.log_writes[0].value = Word(sources.log_writes[0].value.0 + 1);
        owned.bytes.insert((Space::Physical, LOG), sources.log_writes[0].value.0 as u8);
    };
    assert!(matches!(composed_failure(relation_difference(&retained, &owned, stored)), Some(RelationFailure::LogValue { .. })));
    let captured = |owned: &mut Spec| { owned.bytes.insert((Space::Physical, LOG + 1), 0x77); };
    assert!(matches!(composed_failure(relation_difference(&retained, &owned, captured)), Some(RelationFailure::LogCaptured { address: LOG })));
    let dropped = |owned: &mut Spec| { owned.sources.as_mut().unwrap().log_writes.remove(2); };
    assert!(matches!(composed_failure(relation_difference(&retained, &owned, dropped)), Some(RelationFailure::LogUnpaired { .. })));
}

#[test]
fn recorded_accesses_must_explain_the_captured_timer() {
    let (retained, owned) = (timer_steps(100, 200, RETAINED_SITE), timer_steps(160, 300, COMPOSED_SITE));
    let poll = |owned: &mut Spec| owned.sources.as_mut().unwrap().rows[4].value += 1;
    assert!(matches!(composed_failure(relation_difference(&retained, &owned, poll)), Some(RelationFailure::TimerReplayRead { row: 4, .. })));
    let host = |owned: &mut Spec| {
        let sources = owned.sources.as_mut().unwrap();
        let last = *sources.rows.last().unwrap();
        sources.rows.push(TransactionRow { core: None, ..last });
    };
    assert!(matches!(composed_failure(relation_difference(&retained, &owned, host)), Some(RelationFailure::HostAccess { row: 6 })));
    let moved = |owned: &mut Spec| owned.board["bus"]["devices"][0]["state"]["last_tick"] = json!(5);
    assert!(matches!(composed_failure(relation_difference(&retained, &owned, moved)), Some(RelationFailure::TimerSettle { .. })));
}

#[test]
fn a_log_read_is_ordered_against_the_main_timer_start_on_any_core() {
    let mut retained = timer_steps(100, 200, RETAINED_SITE);
    retained.insert(3, on_secondary(get(150, RETAINED_SITE, 0x00)));
    let mut before_start = timer_steps(160, 300, COMPOSED_SITE);
    before_start.insert(0, on_secondary(get(120, COMPOSED_SITE, 0x00)));
    let failure = composed_failure(relation_difference(&retained, &before_start, |_| {}));
    assert!(matches!(failure, Some(RelationFailure::TimerSequence { core: Core::Secondary, index: 0, .. })), "{failure:?}");
    let mut after_start = timer_steps(160, 300, COMPOSED_SITE);
    after_start.insert(3, on_secondary(get(200, COMPOSED_SITE, 0x00)));
    assert_eq!(relation_difference(&retained, &after_start, |_| {}), None);
}

#[test]
fn a_deadline_settle_after_the_last_access_is_explained_by_the_model() {
    let with_application = |base: u64, gap: u64, site: u32| {
        let mut steps = timer_steps(base, gap, site);
        steps.splice(3..3, [put(base + 3, 0x30, 0), put(base + 4, 0x34, 700), put(base + 5, 0x38, 0x03)]);
        steps
    };
    let retained = relation_spec_settled(Role::Retained, &with_application(100, 200, RETAINED_SITE), RETAINED_SITE, Some(820));
    let owned = relation_spec_settled(Role::Composed, &with_application(160, 300, COMPOSED_SITE), COMPOSED_SITE, Some(900));
    assert_eq!(owned.board["bus"]["devices"][0]["state"]["last_tick"], json!(900));
    assert_eq!(relation_verdict(&with_relations(fixture()), &retained, &owned).first_difference, None);
    let later = |owned: &mut Spec| {
        let last = owned.sources.as_ref().unwrap().rows.last().unwrap().tick;
        owned.board["bus"]["devices"][0]["state"]["last_tick"] = json!(last + 5);
    };
    let (retained_steps, owned_steps) = (timer_steps(100, 200, RETAINED_SITE), timer_steps(160, 300, COMPOSED_SITE));
    assert!(matches!(composed_failure(relation_difference(&retained_steps, &owned_steps, later)), Some(RelationFailure::TimerSettle { .. })));
}

#[test]
fn resets_refusals_and_log_entries_must_agree() {
    let (retained, owned) = (timer_steps(100, 200, RETAINED_SITE), timer_steps(160, 300, COMPOSED_SITE));
    let all = |edit: fn(&mut Spec)| {
        let retained_spec = relation_spec(Role::Retained, &retained, RETAINED_SITE);
        let mut owned_spec = relation_spec(Role::Composed, &owned, COMPOSED_SITE);
        edit(&mut owned_spec);
        relation_verdict(&with_relations(fixture()), &retained_spec, &owned_spec).attach_differences.entries
    };
    let failed = |entries: &[Difference], matches: fn(&RelationFailure) -> bool| entries.iter()
        .any(|entry| matches!(entry, Difference::AttachRelation { failure, .. } if matches(failure)));
    assert!(failed(&all(|owned| owned.sources.as_mut().unwrap().reset = SystemTimer::new(vec![0]).unwrap()),
        |failure| matches!(failure, RelationFailure::TimerResetDiffers)));
    assert!(failed(&all(|owned| owned.sources.as_mut().unwrap().rows[4].accepted = false),
        |failure| matches!(failure, RelationFailure::RefusedAccess { row: 4 })));
    assert!(failed(&all(|owned| {
        let writes = &mut owned.sources.as_mut().unwrap().log_writes;
        let (first, second) = (writes[1].value, writes[3].value);
        writes[1].value = second;
        writes[3].value = first;
        set_word(owned, LOG + 4, second.0);
        set_word(owned, LOG + 12, first.0);
    }), |failure| matches!(failure, RelationFailure::TimerSequence { .. })));
    assert!(failed(&all(|owned| owned.sources.as_mut().unwrap().rows[3].pc = Pc(RETAINED_SITE)),
        |failure| matches!(failure, RelationFailure::LogForeignSite { .. })));
    assert!(failed(&all(|owned| { owned.sources.as_mut().unwrap().log_writes.remove(1); }),
        |failure| matches!(failure, RelationFailure::LogEntryValue { .. })));
    assert!(failed(&all(|owned| {
        let words: Vec<(u64, u32)> = owned.sources.as_ref().unwrap().log_writes.iter().map(|write| (write.physical, write.value.0)).collect();
        for (address, _) in &words {
            set_word(owned, *address, 0);
        }
        for write in &mut owned.sources.as_mut().unwrap().log_writes {
            write.physical += 0x20;
        }
        for (address, value) in words {
            set_word(owned, address + 0x20, value);
        }
    }), |failure| matches!(failure, RelationFailure::LogTimestampWords)));
}

#[test]
fn a_later_non_logger_write_makes_a_timestamp_word_compared_again() {
    let overwrite = |spec: &mut Spec, value: u32| {
        let sources = spec.sources.as_mut().unwrap();
        let tick = sources.rows.last().unwrap().tick + 1;
        sources.log_writes.push(LogWrite { core: Core::Primary, pc: Pc(0x5000), physical: LOG, size: 4, value: Word(value), tick });
        set_word(spec, LOG, value);
    };
    let mut retained = relation_spec(Role::Retained, &timer_steps(100, 200, RETAINED_SITE), RETAINED_SITE);
    let mut owned = relation_spec(Role::Composed, &timer_steps(160, 300, COMPOSED_SITE), COMPOSED_SITE);
    overwrite(&mut retained, 0xabcd);
    overwrite(&mut owned, 0xabcd);
    let verdict = relation_verdict(&with_relations(fixture()), &retained, &owned);
    assert_eq!(verdict.first_difference, None);
    assert_eq!(verdict.attach.unwrap().relations[1].timestamps.iter().map(|word| word.address).collect::<Vec<_>>(), [LOG + 8]);
    overwrite(&mut owned, 0xabce);
    let difference = relation_verdict(&with_relations(fixture()), &retained, &owned).first_difference;
    assert!(matches!(difference, Some(Difference::AttachMemory { start: LOG, .. })), "{difference:?}");
}

const LZ_BASE: u64 = 0xfff4_3000;
const LZ_INPUT: u64 = 0x1000;
const LZ_OUTPUT: u32 = 0x4080_0000;

/// Contiguous LZ job counts over one input; with `pending`, the last job is still running on the board. The last
/// start writes `destination` instead of the previous job's end. After the jobs, `reset` writes a stream reset,
/// `count` writes the count register again, and `resize` leaves a MOVRSZ job pending.
#[derive(Clone, Copy)]
struct Jobs<'a> {
    counts: &'a [usize],
    pending: bool,
    reset: bool,
    count: Option<u32>,
    resize: bool,
    destination: Option<u32>,
}

const ONE_JOB: Jobs<'static> = Jobs { counts: &[], pending: false, reset: false, count: None, resize: false, destination: None };

/// Adds `jobs` over `compressed` at `LZ_INPUT` (one job over all of it when `jobs.counts` is empty), the jobs'
/// register writes, and the device's stream and registers as lz.rs leaves them: each fed job's source in one
/// `Stream::feed`, its output at the previous job's end.
fn with_lz(spec: &mut Spec, compressed: &[u8], jobs: Jobs) {
    for (index, byte) in compressed.iter().enumerate() {
        spec.bytes.insert((Space::Physical, LZ_INPUT + index as u64), *byte);
    }
    let whole = [compressed.len()];
    let counts = if jobs.counts.is_empty() { &whole[..] } else { jobs.counts };
    let mut stream = fujimod_emulator_devices::compression::method4::Stream::default();
    let mut regs = [0u32; 64];
    let sources = spec.sources.as_mut().unwrap();
    let mut tick = sources.rows.last().unwrap().tick;
    let mut write = |offset: u64, value: u32| {
        tick += 1;
        sources.rows.push(TransactionRow {
            tick, core: Some(Core::Primary), pc: Pc(0x7000), address: (LZ_BASE + offset) as u32, size: 4,
            access: Access::Write, accepted: true, value,
        });
    };
    let (mut source, mut destination) = (0usize, LZ_OUTPUT);
    for (index, &count) in counts.iter().enumerate() {
        let written = if index == counts.len() - 1 { jobs.destination.unwrap_or(destination) } else { destination };
        for (offset, value) in [(0x40, LZ_INPUT as u32 + source as u32), (0x44, count as u32), (0x48, written), (0x04, 2)] {
            write(offset, value);
            regs[offset as usize / 4] = value;
        }
        regs[0x4c / 4] = 0;
        if !(jobs.pending && index == counts.len() - 1) {
            let produced = stream.feed(&compressed[source..source + count], &mut [0u8; 256]).unwrap() as u32;
            regs[0x4c / 4] = produced;
            destination += produced;
        }
        source += count;
    }
    if jobs.reset {
        write(0x00, 0);
        stream.reset();
        regs[0x4c / 4] = 0;
    }
    if let Some(count) = jobs.count {
        write(0x44, count);
        regs[0x44 / 4] = count;
    }
    let pending = match (jobs.pending && !jobs.reset, jobs.resize) {
        (true, _) => json!({"job": "Lz"}),
        (false, true) => json!({"job": "Resize"}),
        (false, false) => Value::Null,
    };
    spec.board["bus"]["devices"].as_array_mut().unwrap().push(json!({
        "kind": "exr2-lz", "windows": [{"base": LZ_BASE, "size": 256}], "native_dependencies": [],
        "state": {"state": {"stream": serde_json::to_value(&stream).unwrap(), "pending": pending, "regs": regs.to_vec()}},
    }));
}

const RETAINED_LZ_INPUT: [u8; 6] = [3, b'a', b'b', b'c', 0x80, 0x36];
const OWNED_LZ_INPUT: [u8; 10] = [9, b'a', b'b', b'c', b'a', b'b', b'c', b'a', b'b', b'c'];

/// Both roles with LZ jobs over inputs of different lengths: `retained` in the retained role, `owned` in the composed.
fn lz_roles_with(retained_jobs: Jobs, owned_jobs: Jobs) -> (Spec, Spec) {
    let mut retained = relation_spec(Role::Retained, &timer_steps(100, 200, RETAINED_SITE), RETAINED_SITE);
    let mut owned = relation_spec(Role::Composed, &timer_steps(160, 300, COMPOSED_SITE), COMPOSED_SITE);
    with_lz(&mut retained, &RETAINED_LZ_INPUT, retained_jobs);
    with_lz(&mut owned, &OWNED_LZ_INPUT, owned_jobs);
    (retained, owned)
}

fn lz_roles(owned_jobs: Jobs) -> (Spec, Spec) { lz_roles_with(ONE_JOB, owned_jobs) }

/// The relations fixture with `lz` and a copy allowance pinning each role's LZ input.
fn lz_contract(lz: Value) -> Value {
    let padded = |input: &[u8]| { let mut bytes = input.to_vec(); bytes.resize(16, 0); Sha256::of(&bytes) };
    let mut value = with_relations(fixture());
    value["guard"]["attach_state"]["relations"]["lz"] = lz;
    attach_rules(&mut value)["allowances"] = json!([{
        "label": "lz-input", "kind": "copy", "space": {"target": "physical"}, "start": LZ_INPUT, "end": LZ_INPUT + 16, "reason": "r",
        "source": {"dat": {"offset": 0, "transform": "identity"}}, "canonical_sha256": padded(&RETAINED_LZ_INPUT), "composed_sha256": padded(&OWNED_LZ_INPUT),
    }]);
    value
}

#[test]
fn the_lz_tallies_follow_each_role_s_pinned_input() {
    let (retained, owned) = lz_roles(ONE_JOB);
    let verdict = relation_verdict(&lz_contract(json!({"citation": "lz.rs"})), &retained, &owned);
    assert_eq!(verdict.first_difference, None, "{:?}", verdict.attach_differences);
    let tallies = verdict.attach.unwrap().relations.iter().map(|role| role.lz.as_ref().map(|lz| (lz.literals, lz.matches))).collect::<Vec<_>>();
    assert_eq!(tallies, [Some((1, 1)), Some((1, 0))]);
    let without = relation_verdict(&lz_contract(Value::Null), &retained, &owned).first_difference;
    assert!(matches!(without, Some(Difference::AttachBoard { .. })), "{without:?}");
    let mut overlap = lz_contract(json!({"citation": "lz.rs"}));
    attach_rules(&mut overlap)["board"] = json!([{"pointer": "/bus/devices/1/state/state/stream/literals", "kind": "declared", "reason": "r", "citation": "c"}]);
    let entries = relation_verdict(&overlap, &retained, &owned).attach_differences.entries;
    assert!(entries.iter().any(|entry| matches!(entry, Difference::AttachRelation { failure: RelationFailure::RelationOverlap { .. }, .. })));
}

/// `role`'s relation failures, the board differences and `role`'s derived LZ registers, with `edit` applied to that
/// role's captured LZ registers.
fn lz_role_outcome(role: Role, jobs: [Jobs; 2], edit: impl FnOnce(&mut Vec<Value>)) -> (Vec<RelationFailure>, Vec<String>, Option<LzRegisters>) {
    let (mut retained, mut owned) = lz_roles_with(jobs[0], jobs[1]);
    let spec = match role { Role::Retained => &mut retained, Role::Composed => &mut owned };
    let devices = spec.board["bus"]["devices"].as_array_mut().unwrap();
    edit(devices.last_mut().unwrap()["state"]["state"]["regs"].as_array_mut().unwrap());
    let verdict = relation_verdict(&lz_contract(json!({"citation": "lz.rs"})), &retained, &owned);
    let entries = &verdict.attach_differences.entries;
    let failures = entries.iter().filter_map(|entry| match entry {
        Difference::AttachRelation { role: failed, failure } if *failed == role => Some(failure.clone()),
        _ => None,
    }).collect();
    let boards = entries.iter().filter_map(|entry| match entry {
        Difference::AttachBoard { pointer, .. } => Some(pointer.0.clone()),
        _ => None,
    }).collect();
    let index = match role { Role::Retained => 0, Role::Composed => 1 };
    let registers = verdict.attach.and_then(|attach| attach.relations.get(index).and_then(|role| role.lz.as_ref().map(|lz| lz.registers.clone())));
    (failures, boards, registers)
}

fn lz_register_outcome(owned_jobs: Jobs, edit: impl FnOnce(&mut Vec<Value>)) -> (Vec<RelationFailure>, Vec<String>, Option<LzRegisters>) {
    lz_role_outcome(Role::Composed, [ONE_JOB, owned_jobs], edit)
}

const LZ_REGISTERS: [&str; 3] = ["/bus/devices/1/state/state/regs/17", "/bus/devices/1/state/state/regs/18", "/bus/devices/1/state/state/regs/19"];

#[test]
fn the_lz_count_destination_and_output_registers_follow_each_role_s_jobs() {
    let (failures, boards, registers) = lz_register_outcome(ONE_JOB, |_| {});
    assert_eq!((failures, boards), (vec![], vec![]));
    assert_eq!(registers, Some(LzRegisters { count: Some(Word(10)), destination: Some(Word(LZ_OUTPUT)), output: Some(Word(9)) }));
    let split = Jobs { counts: &[4, 6], ..ONE_JOB };
    for jobs in [split, Jobs { resize: true, ..split }, Jobs { pending: true, ..split }] {
        let (failures, boards, _) = lz_register_outcome(jobs, |_| {});
        assert_eq!(failures, []);
        assert!(!boards.iter().any(|pointer| LZ_REGISTERS.contains(&pointer.as_str())), "{boards:?}");
    }
}

/// The retained input split inside its match token: the first job emits `abc`, the last the 6-byte match.
#[test]
fn the_destination_and_output_count_the_output_before_and_of_the_last_job() {
    let split = Jobs { counts: &[5, 1], ..ONE_JOB };
    let (failures, _, _) = lz_role_outcome(Role::Retained, [split, ONE_JOB], |regs| {
        assert_eq!((regs[18].as_u64(), regs[19].as_u64()), (Some(u64::from(LZ_OUTPUT) + 3), Some(6)));
    });
    assert_eq!(failures, []);
    let written = Jobs { destination: Some(LZ_OUTPUT), ..split };
    let (failures, _, _) = lz_role_outcome(Role::Retained, [written, ONE_JOB], |_| {});
    assert!(matches!(failures.as_slice(), [RelationFailure::LzRegister { leaf, .. }] if leaf.0 == LZ_REGISTERS[1]), "{failures:?}");
}

#[test]
fn a_captured_lz_register_off_its_jobs_refuses() {
    for register in [17, 18, 19] {
        let (failures, _, _) = lz_register_outcome(Jobs { counts: &[4, 6], ..ONE_JOB }, |regs| {
            regs[register] = json!(regs[register].as_u64().unwrap() + 1);
        });
        assert!(matches!(failures.as_slice(), [RelationFailure::LzRegister { leaf, .. }] if leaf.0 == LZ_REGISTERS[register - 17]), "{failures:?}");
    }
    let (failures, _, _) = lz_register_outcome(Jobs { counts: &[4, 6], pending: true, ..ONE_JOB }, |regs| regs[19] = json!(1));
    assert!(matches!(failures.as_slice(), [RelationFailure::LzRegister { .. }]), "{failures:?}");
}

#[test]
fn an_lz_register_the_jobs_do_not_fix_is_compared_across_roles() {
    let (failures, boards, _) = lz_register_outcome(Jobs { reset: true, ..ONE_JOB }, |_| {});
    assert_eq!(failures, []);
    assert!(boards.contains(&LZ_REGISTERS[0].to_owned()) && !boards.contains(&LZ_REGISTERS[2].to_owned()), "{boards:?}");
    let (failures, boards, _) = lz_register_outcome(Jobs { count: Some(7), ..ONE_JOB }, |_| {});
    assert_eq!(failures, []);
    assert_eq!(boards, [LZ_REGISTERS[0]]);
}

#[test]
fn a_board_entry_on_an_lz_register_overlaps_the_relation() {
    let (retained, owned) = lz_roles(ONE_JOB);
    let mut overlap = lz_contract(json!({"citation": "lz.rs"}));
    attach_rules(&mut overlap)["board"] = json!([{"pointer": LZ_REGISTERS[2], "kind": "exact", "retained": 1, "composed": 2, "reason": "r"}]);
    let entries = relation_verdict(&overlap, &retained, &owned).attach_differences.entries;
    assert!(entries.iter().any(|entry| matches!(entry, Difference::AttachRelation { failure: RelationFailure::RelationOverlap { .. }, .. })));
}

const SUBMITTED: u64 = 0x42a4;

fn submitted_contract(retained_length: u32, composed_length: u32) -> Value {
    lz_contract(json!({"citation": "lz.rs", "submitted": {
        "address": SUBMITTED, "citation": "boot-40048.dis 0x3ffc", "record_length": {"retained": retained_length, "composed": composed_length},
    }}))
}

/// Each role's jobs and record length, with each word as `words` sets it.
fn submitted_verdict(words: [u32; 2], lengths: [u32; 2], owned_jobs: Jobs) -> Verdict {
    let (mut retained, mut owned) = lz_roles(owned_jobs);
    set_word(&mut retained, SUBMITTED, words[0]);
    set_word(&mut owned, SUBMITTED, words[1]);
    relation_verdict(&submitted_contract(lengths[0], lengths[1]), &retained, &owned)
}

const SUBMITTED_LENGTHS: [u32; 2] = [RETAINED_LZ_INPUT.len() as u32 + 0x14, OWNED_LZ_INPUT.len() as u32 + 0x14];

#[test]
fn the_submitted_word_differs_between_roles_when_each_matches_its_jobs_and_record() {
    let verdict = submitted_verdict([6, 10], SUBMITTED_LENGTHS, ONE_JOB);
    assert_eq!(verdict.first_difference, None, "{:?}", verdict.attach_differences);
    let words = verdict.attach.unwrap().relations.iter().map(|role| role.lz.as_ref().and_then(|lz| lz.submitted)).collect::<Vec<_>>();
    assert_eq!(words, [Some(Word(6)), Some(Word(10))]);
}

/// The board's pending job and unfed stream history still differ between roles; only the relation is asserted.
#[test]
fn a_pending_last_job_counts_in_the_submitted_word() {
    let (failures, compared) = submitted_failures(&submitted_verdict([6, 10], SUBMITTED_LENGTHS, Jobs { counts: &[4, 6], pending: true, ..ONE_JOB }));
    assert_eq!(failures, []);
    assert!(!compared);
    let (failures, compared) = submitted_failures(&submitted_verdict([6, 4], SUBMITTED_LENGTHS, Jobs { counts: &[4, 6], pending: true, ..ONE_JOB }));
    assert_eq!(failures, [RelationFailure::LzSubmittedDrain { captured: Word(4), started: 10, last: 6 }]);
    assert!(compared);
}

/// The composed role's relation failures, and whether the submitted word reached the memory comparison.
fn submitted_failures(verdict: &Verdict) -> (Vec<RelationFailure>, bool) {
    let entries = &verdict.attach_differences.entries;
    let failures = entries.iter().filter_map(|entry| match entry {
        Difference::AttachRelation { role: Role::Composed, failure } => Some(failure.clone()),
        _ => None,
    }).collect();
    let compared = entries.iter().any(|entry| matches!(entry, Difference::AttachMemory { start: SUBMITTED, .. }));
    (failures, compared)
}

#[test]
fn a_submitted_word_off_its_jobs_and_record_is_compared() {
    let (failures, compared) = submitted_failures(&submitted_verdict([6, 11], SUBMITTED_LENGTHS, ONE_JOB));
    assert_eq!(failures, [RelationFailure::LzSubmitted { captured: Word(11), started: 10, stream: 10 }]);
    assert!(compared);
}

#[test]
fn a_record_length_that_disagrees_with_the_job_total_refuses() {
    let (failures, compared) = submitted_failures(&submitted_verdict([6, 10], [SUBMITTED_LENGTHS[0], SUBMITTED_LENGTHS[1] + 1], ONE_JOB));
    assert_eq!(failures, [RelationFailure::LzSubmitted { captured: Word(10), started: 10, stream: 11 }]);
    assert!(compared);
}

#[test]
fn a_last_job_missing_from_a_complete_stream_is_the_drain_step() {
    let (failures, compared) = submitted_failures(&submitted_verdict([6, 0], SUBMITTED_LENGTHS, ONE_JOB));
    assert_eq!(failures, [RelationFailure::LzSubmittedDrain { captured: Word(0), started: 10, last: 10 }]);
    assert!(compared);
    let (failures, compared) = submitted_failures(&submitted_verdict([6, 4], SUBMITTED_LENGTHS, Jobs { counts: &[4, 6], ..ONE_JOB }));
    assert_eq!(failures, [RelationFailure::LzSubmittedDrain { captured: Word(4), started: 10, last: 6 }]);
    assert!(compared);
    let (failures, _) = submitted_failures(&submitted_verdict([6, 3], SUBMITTED_LENGTHS, Jobs { counts: &[4, 6], ..ONE_JOB }));
    assert_eq!(failures, [RelationFailure::LzSubmitted { captured: Word(3), started: 10, stream: 10 }]);
}

#[test]
fn a_submitted_word_must_be_aligned_and_clear_of_allowances() {
    for (address, length) in [(SUBMITTED + 2, 0x20), (LZ_INPUT, 0x20), (LOG, 0x20), (SUBMITTED, 0x13), (0x2000_0000, 0x20), (u64::MAX - 1, 0x20)] {
        let mut value = submitted_contract(length, length);
        value["guard"]["attach_state"]["relations"]["lz"]["submitted"]["address"] = json!(address);
        assert!(matches!(parse(&value), Err(CatalogRefusal::AttachState(AttachRuleRefusal::LzSubmittedWord))), "{address:#x}");
    }
}

#[test]
fn a_board_entry_over_a_relation_leaf_refuses() {
    let retained = relation_spec(Role::Retained, &timer_steps(100, 200, RETAINED_SITE), RETAINED_SITE);
    let owned = relation_spec(Role::Composed, &timer_steps(160, 300, COMPOSED_SITE), COMPOSED_SITE);
    let mut value = with_relations(fixture());
    attach_rules(&mut value)["board"] = json!([{"pointer": "/bus/devices/0/state/count", "kind": "declared", "reason": "r", "citation": "c"}]);
    let difference = relation_verdict(&value, &retained, &owned).first_difference;
    let differences = relation_verdict(&value, &retained, &owned).attach_differences.entries;
    assert!(difference.is_some());
    assert!(differences.iter().any(|entry| matches!(entry, Difference::AttachRelation { failure: RelationFailure::RelationOverlap { .. }, .. })));
}

#[test]
fn log_sites_and_spans_are_checked_at_parse() {
    let mut value = with_relations(fixture());
    value["guard"]["attach_state"]["relations"]["log"]["sites"][0]["store_pc"] = json!(RETAINED_SITE + 8);
    assert!(matches!(parse(&value), Err(CatalogRefusal::AttachState(AttachRuleRefusal::LogSite(_)))));
    let mut value = with_relations(fixture());
    value["guard"]["attach_state"]["relations"]["log"]["span"] = json!({"start": LOG + 2, "end": LOG + 0x80});
    assert!(matches!(parse(&value), Err(CatalogRefusal::AttachState(AttachRuleRefusal::LogSpan))));
    let mut value = with_relations(fixture());
    value["guard"]["attach_state"]["relations"]["log"]["sites"][2]["role"] = json!("retained");
    value["guard"]["attach_state"]["relations"]["log"]["sites"][2]["read_pc"] = json!(RETAINED_SITE);
    value["guard"]["attach_state"]["relations"]["log"]["sites"][2]["store_pc"] = json!(RETAINED_SITE + 4);
    assert!(matches!(parse(&value), Err(CatalogRefusal::AttachState(AttachRuleRefusal::LogSite(_)))));
}

#[test]
fn the_census_passes_only_on_the_declared_rows_and_their_choice_maps() {
    use fujimod_xm1::composed_mutations::{CensusMutation, CensusRow, ChoiceMap, MappingKind, Origin, census_difference};
    let contract = parse(&fixture()).unwrap();
    let declared = contract.declared(Role::Composed);
    let map = |core| ChoiceMap { core, address: 0, physical: 0, bytes: 0x4000, permissions: 7 };
    let maps = vec![vec![vec![], vec![map(Core::Primary), map(Core::Secondary)]]];
    let row = |core, mutation, writer| CensusRow { core: Some(core), mutation, pc: Some(4), cpsr: None, writer, instructions: Some(5) };
    let commit = row(Core::Primary, CensusMutation::DecoderCommit { decoder: 0, previous: 0, next: 1 }, Some(0x8000_22a8));
    let replaced = |core| row(core, CensusMutation::Mapping {
        operation: MappingKind::ReplacePhysical, address: 0, physical: Some(0), bytes: 0x4000, permissions: Some(7),
    }, None);
    let tcm = |writer| row(Core::Primary, CensusMutation::TcmRegion {
        bank: Some(Bank::Instruction), previous: 0xffff_8011, next: 0x7000_0011, origin: Origin::Guest,
    }, Some(writer));
    let passing = vec![commit, replaced(Core::Primary), replaced(Core::Secondary), tcm(0x605c)];
    assert_eq!(census_difference(declared, &maps, &passing), Ok(None));
    let interleaved = vec![tcm(0x605c), commit, replaced(Core::Primary), replaced(Core::Secondary)];
    assert_eq!(census_difference(declared, &maps, &interleaved), Ok(None));
    let failing = [
        vec![commit, replaced(Core::Primary), tcm(0x605c)],
        vec![commit, replaced(Core::Primary), replaced(Core::Secondary), tcm(0x6060)],
        vec![replaced(Core::Primary), commit, replaced(Core::Secondary), tcm(0x605c)],
        vec![commit, replaced(Core::Primary), replaced(Core::Secondary), tcm(0x605c),
            row(Core::Secondary, CensusMutation::HostWrite { address: 0, bytes: 4 }, None)],
    ];
    for rows in failing {
        assert!(matches!(census_difference(declared, &maps, &rows), Ok(Some(_))), "{rows:?}");
    }
}

#[test]
fn rom_declarations_bind_the_boot_rom_words_and_the_run_graph_decoder() {
    use fujimod_xm1::composed_mutations::{ROM_SELECT_POOL, ROM_SELECT_STORE, check_rom_declarations, choice_maps};
    use fujimod_xm1::run::RunDocument;
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap();
    let mut rom = vec![0u8; document.boot_rom.bytes];
    for (address, word) in [ROM_SELECT_STORE, ROM_SELECT_POOL] {
        let offset = (address - 0x8000_0000) as usize;
        rom[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    let contract = parse(&fixture()).unwrap();
    let declared = contract.declared(Role::Composed).to_vec();
    assert_eq!(check_rom_declarations(&document, &rom, &declared), Ok(()));
    let mut changed = rom.clone();
    changed[0x22a8] ^= 1;
    assert!(check_rom_declarations(&document, &changed, &declared).is_err());
    let mut value = fixture();
    value["guard"]["declared"]["composed"][0]["writer"] = json!(0x8000_22acu32);
    assert!(check_rom_declarations(&document, &rom, parse(&value).unwrap().declared(Role::Composed)).is_err());
    value["guard"]["declared"]["composed"][0]["writer"] = json!(0x8000_22a8u32);
    value["guard"]["declared"]["composed"][0]["previous"] = json!(1);
    value["guard"]["declared"]["composed"][0]["next"] = json!(0);
    assert!(check_rom_declarations(&document, &rom, parse(&value).unwrap().declared(Role::Composed)).is_err());
    let maps = choice_maps(&document).unwrap();
    assert!(maps.first().and_then(|choices| choices.get(1)).is_some_and(|choice| !choice.is_empty()));
}
