use fujimod_emulator::board::observation::{
    CompositeGuardConfig, DestinationPolicy, GuardCore, GuardRange, GuardSpan, GuardTarget, OwnedPcPolicy, SourcePolicy,
    TcmBank, WatchKinds, WatchRange, WriteSource,
};
use fujimod_xm1::composed_catalog::{
    CatalogRefusal, Confinement, Contract, Core, GuardBound, Label, Pc, PieceKind, Role, Sha256, UnconfinedCode,
};
use fujimod_xm1::composed_receipt::{
    ByteRange, CarrierField, CarrierWrites, Difference, GuardObservation, GuardState, HexBytes, Identity, IdentityField, LabelCount,
    MenuObservation, Outcome, ReadCapture, Receipt, ReceiptKind, Register, Row, SchemaOne, VerdictKind, WatchRow, Word, WriteKind,
    Writer, WriterCount, carrier_writes, compare,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const FIXTURE: &str = include_str!("data/composed-contract-v1.json");
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
    assert!(matches!(refused(|v| allow(v, json!([{"target": "physical", "core": 0, "start": 0, "end": 16}]))),
        CatalogRefusal::WriteRangeShape(_)));
    let many: Vec<Value> = (0..250u64).map(|index| json!({"target": "physical", "start": 0x100000 + 16 * index, "end": 0x100000 + 16 * index + 8})).collect();
    assert!(matches!(refused(|v| allow(v, Value::Array(many))), CatalogRefusal::GuardBound(GuardBound::Ranges(_))));
    let spans: Vec<Value> = (0..65u64).map(|index| json!({"start": 263216 + 8 * index, "end": 263216 + 8 * index + 4})).collect();
    assert_eq!(refused(|v| v["pieces"][1]["catalog"]["owned_code"] = Value::Array(spans)),
        CatalogRefusal::GuardBound(GuardBound::PcSpans(260)));
    let mut value = fixture();
    value["pieces"][1]["catalog"]["boundaries"][1]["cores"] = json!([0, 1]);
    allow(&mut value, json!([{"target": "tcm", "core": 1, "bank": "data", "start": 0, "end": 16}]));
    assert!(parse(&value).is_ok());
}

#[test]
fn core_sets_and_unenumerated_writes_refuse_when_malformed() {
    for cores in [json!([]), json!([1, 0]), json!([0, 0]), json!([2])] {
        assert!(matches!(refused(|v| v["pieces"][1]["catalog"]["boundaries"][1]["cores"] = cores), CatalogRefusal::Shape(_)));
    }
    assert_eq!(refused(|v| v["pieces"][1]["catalog"]["boundaries"][0]["cores"] = json!([0, 1])),
        CatalogRefusal::AttachCores(label("primary-publisher/entry-core0")));
    assert!(matches!(refused(|v| v["pieces"][0]["catalog"]["writes"]["allow"] = json!([{"target": "physical", "start": 0, "end": 16}])),
        CatalogRefusal::UnenumeratedAllow(_)));
    assert!(matches!(refused(|v| v["pieces"][0]["catalog"]["writes"]["premise"] = Value::Null), CatalogRefusal::WritesPremise(_)));
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

fn span(start: u64, end: u64) -> GuardSpan { GuardSpan::new(start, end).unwrap() }

fn core(index: usize) -> GuardCore { GuardCore::new(index).unwrap() }

fn physical(start: u64, end: u64) -> GuardRange { GuardRange { target: GuardTarget::Physical, span: span(start, end) } }

fn tcm(owner: usize, bank: TcmBank, start: u64, end: u64) -> GuardRange {
    GuardRange { target: GuardTarget::Tcm { core: core(owner), bank }, span: span(start, end) }
}

fn allow(ranges: Vec<GuardRange>) -> DestinationPolicy { DestinationPolicy { allow: ranges, forbid: vec![] } }

fn stock(owner: usize) -> DestinationPolicy {
    allow(vec![physical(0, END), tcm(owner, TcmBank::Data, 0, END), tcm(owner, TcmBank::Instruction, 0, END)])
}

fn expected(owned: [Option<OwnedPcPolicy>; 2]) -> CompositeGuardConfig {
    let [first, second] = owned;
    CompositeGuardConfig { sources: vec![
        SourcePolicy { source: WriteSource::Cpu(core(0)), destinations: stock(0), owned_pc: first },
        SourcePolicy { source: WriteSource::Cpu(core(1)), destinations: stock(1), owned_pc: second },
        SourcePolicy { source: WriteSource::CoreHost(core(0)), destinations: stock(0), owned_pc: None },
        SourcePolicy { source: WriteSource::CoreHost(core(1)), destinations: stock(1), owned_pc: None },
        SourcePolicy { source: WriteSource::PhysicalHost, destinations: allow(vec![physical(0, END)]), owned_pc: None },
        SourcePolicy { source: WriteSource::ModelBus, destinations: allow(vec![physical(0, END)]), owned_pc: None },
    ] }
}

type Ranges = Vec<String>;

/// Each source's policy with every range and span list sorted.
fn normal(config: &CompositeGuardConfig) -> BTreeMap<String, (Ranges, Ranges, Option<(Vec<(u64, u64)>, Ranges, Ranges)>)> {
    let sorted = |ranges: &[GuardRange]| { let mut out: Ranges = ranges.iter().map(|range| format!("{range:?}")).collect(); out.sort(); out };
    let normalized: BTreeMap<_, _> = config.sources.iter().map(|policy| (format!("{:?}", policy.source), (
        sorted(&policy.destinations.allow), sorted(&policy.destinations.forbid),
        policy.owned_pc.as_ref().map(|owned| {
            let mut spans: Vec<_> = owned.spans.iter().map(|span| (span.start(), span.end())).collect();
            spans.sort();
            (spans, sorted(&owned.destinations.allow), sorted(&owned.destinations.forbid))
        }),
    ))).collect();
    assert_eq!(normalized.len(), config.sources.len());
    normalized
}

const ALIAS: u64 = 0x4000_0000;

fn spellings(start: u64, end: u64) -> [GuardSpan; 2] { [span(start, end), span(ALIAS + start, ALIAS + end)] }

fn publisher_spans() -> Vec<GuardSpan> { spellings(263216, 263924).to_vec() }

fn first_stage() -> PieceKind { "first-stage".to_owned().try_into().unwrap() }

#[test]
fn unenumerated_code_is_unconfined_and_enumerated_code_is_held_to_its_allows() {
    let contract = parse(&fixture()).unwrap();
    let owned = || Some(OwnedPcPolicy { spans: publisher_spans(), destinations: allow(vec![physical(318176, 323852), physical(404564, 406624)]) });
    assert_eq!(normal(&contract.guard_config(Role::Composed)), normal(&expected([owned(), owned()])));
    assert_eq!(contract.confinement(Role::Composed), Confinement::UnionOfEnumeratedAllows);
    assert_eq!(contract.unconfined(Role::Composed), [UnconfinedCode { piece: first_stage(), start: 24576, end: 25772 }]);
    assert_eq!(normal(&contract.guard_config(Role::Retained)), normal(&expected([None, None])));
    assert_eq!(contract.confinement(Role::Retained), Confinement::Stock);
    assert!(contract.unconfined(Role::Retained).is_empty());
}

#[test]
fn enumerated_pieces_share_the_union_of_their_allows_with_each_core_keeping_its_own_tcm() {
    let mut value = fixture();
    value["pieces"][0]["catalog"]["writes"] = json!({
        "allow": [{"target": "physical", "start": 0, "end": 24576}, {"target": "tcm", "core": 0, "bank": "instruction", "start": 0, "end": 65536}],
        "basis": "premised", "premise": "first-stage scratch",
    });
    let contract = parse(&value).unwrap();
    let mut spans = spellings(24576, 25772).to_vec();
    spans.extend(publisher_spans());
    let owned = |ranges| Some(OwnedPcPolicy { spans: spans.clone(), destinations: allow(ranges) });
    let physical_union = || vec![physical(0, 24576), physical(318176, 323852), physical(404564, 406624)];
    assert_eq!(normal(&contract.guard_config(Role::Composed)), normal(&expected([
        owned([physical_union(), vec![tcm(0, TcmBank::Instruction, 0, 65536)]].concat()),
        owned(physical_union()),
    ])));
    assert!(contract.unconfined(Role::Composed).is_empty());
}

#[test]
fn only_unenumerated_pieces_leave_every_cpu_on_the_stock_policy() {
    let mut value = fixture();
    value["pieces"][1]["catalog"]["writes"] = json!({"allow": [], "basis": "unenumerated", "premise": "not enumerated"});
    let contract = parse(&value).unwrap();
    assert_eq!(normal(&contract.guard_config(Role::Composed)), normal(&expected([None, None])));
    assert_eq!(contract.confinement(Role::Composed), Confinement::Stock);
    assert_eq!(contract.unconfined(Role::Composed).len(), 2);
}

#[test]
fn an_owned_span_already_listed_in_both_spellings_yields_the_same_policy() {
    let mut value = fixture();
    value["pieces"][1]["catalog"]["owned_code"] = json!([
        {"start": 263216, "end": 263924}, {"start": ALIAS + 263216, "end": ALIAS + 263924}]);
    let both = parse(&value).unwrap();
    let single = parse(&fixture()).unwrap();
    assert_eq!(normal(&both.guard_config(Role::Composed)), normal(&single.guard_config(Role::Composed)));
}

#[test]
fn guard_config_translates_alias_destinations_to_physical() {
    let mut value = fixture();
    value["pieces"][1]["catalog"]["writes"]["allow"] = json!([
        {"target": "physical", "start": 318176, "end": 323852},
        {"target": "physical", "start": ALIAS + 404564, "end": ALIAS + 406624},
    ]);
    let contract = parse(&value).unwrap();
    let owned = || Some(OwnedPcPolicy { spans: publisher_spans(), destinations: allow(vec![physical(318176, 323852), physical(404564, 406624)]) });
    assert_eq!(normal(&contract.guard_config(Role::Composed)), normal(&expected([owned(), owned()])));
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
        schema: SchemaOne,
        kind: ReceiptKind::ComposedStartupReceipt,
        role,
        identity: Identity {
            run: digest('a'), update: contract.identities(role).dat_sha256.clone(), boot_rom: digest('b'),
            observer: digest('c'), contract: contract_digest.clone(),
        },
        confinement: contract.confinement(role),
        unconfined: contract.unconfined(role),
        outcome: Outcome::Observed,
        error: None,
        machine_failure: None,
        instructions: 80_000_000,
        rows,
        counts,
        off_core: vec![],
        unguarded: vec![],
        guard: Some(GuardObservation {
            attach_instructions: 900, end_instructions: 250_000_000, status: GuardState::Ended, first_fault: None,
            write_checks: 12, mutation_attempts: 0, carrier_writes: carriers(contract, &stock_writes()), watch_violation: None,
            watch_end_error: None,
        }),
        menu: MenuObservation { erased_settings: true, shooting: true },
    }
}

fn verdict(value: &Value, edit: impl FnOnce(&mut Receipt)) -> Option<Difference> {
    let bytes = serde_json::to_vec(value).unwrap();
    let contract = Contract::parse(&bytes).unwrap();
    let digest = Sha256::of(&bytes);
    let retained = receipt(&contract, &digest, Role::Retained);
    let mut owned = receipt(&contract, &digest, Role::Composed);
    edit(&mut owned);
    let roundtrip: Receipt = serde_json::from_slice(&serde_json::to_vec(&owned).unwrap()).unwrap();
    let result = compare(&contract, &digest, &retained, &roundtrip);
    assert_eq!(result.verdict == VerdictKind::Equal, result.first_difference.is_none());
    result.first_difference
}

#[test]
fn equal_receipts_compare_equal() {
    assert_eq!(verdict(&fixture(), |_| {}), None);
}

#[test]
fn owned_only_boundaries_and_unguarded_occurrences_do_not_differ() {
    assert_eq!(verdict(&fixture(), |owned| {
        owned.rows.insert(0, row("first-stage/entry", [9, 9, 9, 9], 0x1000, vec![]));
        owned.unguarded.push(LabelCount { label: label("first-stage/entry"), core: Core::Primary, total: 1 });
    }), None);
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
        compare(&contract, &digest, &retained, &owned).first_difference
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
fn host_plan_lists_each_roles_armed_sites_guard_and_unconfined_code() {
    use fujimod_xm1::composed_catalog::{ArmedSite, PlanSpan, RangePlan, SourcePlan};
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
    assert_eq!(composed.confinement, Confinement::UnionOfEnumeratedAllows);
    assert_eq!(composed.unconfined, [UnconfinedCode { piece: first_stage(), start: 24576, end: 25772 }]);
    assert!(retained.unconfined.is_empty());
    let cpu1 = composed.guard.iter().find(|policy| policy.source == SourcePlan::Cpu { core: 1 }).unwrap();
    assert!(cpu1.allow.contains(&RangePlan::Tcm { core: 1, bank: fujimod_xm1::composed_catalog::Bank::Instruction, start: 0, end: END }));
    let owned = cpu1.owned_pc.as_ref().unwrap();
    let mut spans = owned.spans.clone();
    spans.sort();
    assert_eq!(spans, [PlanSpan { start: 263216, end: 263924 }, PlanSpan { start: ALIAS + 263216, end: ALIAS + 263924 }]);
    assert_eq!(owned.allow, [RangePlan::Physical { start: 318176, end: 323852 }, RangePlan::Physical { start: 404564, end: 406624 }]);
    assert_eq!(composed.guard.len(), 6);
    assert!(retained.guard.iter().all(|policy| policy.owned_pc.is_none()));
    assert!(serde_json::to_value(&plan).is_ok());
}
