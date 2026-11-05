use fujimod_xm1::{
    modal_lifetime::{CurrentState, LifetimeConfig, LifetimeFailure, LifetimeMemory, LifetimeMonitor,
        LifetimePhase, LifetimeRequest, Publication, VfpOwnerState, Word, WriteAttempt, WriteSource},
    modal_permit::{CoreId, Cpsr},
    modal_task::{MemoryError, PhysicalExtent, StackExtent, TaskId, ValidatedTaskMemory, VfpOwner, source_layouts, vfp_owner},
    provenance::Guest,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const INSTALL: u32 = 0x0100_0000;
const INSTALL_WRITER: u32 = INSTALL + 4;
const ROLLBACK: u32 = INSTALL + 8;
const ROLLBACK_WRITER: u32 = INSTALL + 12;
const STR: u32 = 0xe581_3000;
const RESIDENT: u32 = INSTALL + 0x8000;
const SETTINGS: u32 = 0x0096_cc64;

#[derive(Default)]
struct Memory(BTreeMap<u32, u8>);
impl Memory {
    fn store(&mut self, address: u32, bytes: &[u8]) {
        self.0.extend(bytes.iter().enumerate().map(|(index, byte)| (address + index as u32, *byte)));
    }
}
impl LifetimeMemory for Memory {
    fn read(&self, address: Guest, output: &mut [u8]) -> Result<(), LifetimeFailure> {
        for (index, byte) in output.iter_mut().enumerate() {
            *byte = self.0.get(&(address.0 + index as u32)).copied().unwrap_or(0);
        }
        Ok(())
    }
}
fn request() -> Value {
    let words = [0xe92d_4010u32, STR, 0xe92d_4010, STR];
    let bytes: Vec<_> = words.into_iter().flat_map(u32::to_le_bytes).collect();
    let publications: Vec<_> = (0..223).map(|index| json!({
        "address": match index { 0 => 0x00cf_59e4u32, 1 => 0x0120_0008, 2 => 0x0120_0004, _ => 0x0120_0000 + index * 4 },
        "old": 0, "new": index + 1,
    })).collect();
    json!({
        "runtime": [INSTALL, INSTALL + 0x10000], "resident": [RESIDENT, RESIDENT + 1024],
        "readonly": [{"start": INSTALL, "bytes": bytes}],
        "install": {"entry": {"pc": INSTALL, "word": words[0]}, "writer": {"pc": INSTALL_WRITER, "word": STR}},
        "rollback": {"entry": {"pc": ROLLBACK, "word": words[2]}, "writer": {"pc": ROLLBACK_WRITER, "word": STR}},
        "publications": publications,
    })
}
fn config(value: Value) -> Result<LifetimeConfig, LifetimeFailure> {
    let request: LifetimeRequest = serde_json::from_value(value).unwrap();
    request.into_config(vec![
        PhysicalExtent::new(Guest(0x40000), 0xdae0).unwrap(),
        PhysicalExtent::new(Guest(0xd0000), 0xdaa0).unwrap(),
    ])
}
fn fixture() -> (LifetimeMonitor, Memory) {
    let config = config(request()).unwrap();
    let mut memory = Memory::default();
    for (extent, bytes) in config.readonly_images() { memory.store(extent.start().0, bytes); }
    memory.store(SETTINGS, &[0x5a; 450]);
    (LifetimeMonitor::new(config, &memory).unwrap(), memory)
}
fn entry(monitor: &mut LifetimeMonitor, core: usize, rollback: bool) -> Result<(), LifetimeFailure> {
    monitor.observe_entry(CoreId(core), Guest(if rollback { ROLLBACK } else { INSTALL }), Word(0xe92d_4010))
}
fn prepare(monitor: &mut LifetimeMonitor, core: usize, row: Publication, reverse: bool) -> Result<(), LifetimeFailure> {
    let mut registers = [0; 13];
    registers[1] = row.address.0;
    registers[3] = if reverse { row.old.0 } else { row.new.0 };
    monitor.prepare_store(CoreId(core), Guest(if reverse { ROLLBACK_WRITER } else { INSTALL_WRITER }), Word(STR), &registers,
        if reverse { row.new } else { row.old }, Cpsr(0x13))
}
fn publication_row(row: Publication, core: usize, reverse: bool) -> Value {
    let before = if reverse { row.new } else { row.old };
    let value = if reverse { row.old } else { row.new };
    json!({"source":"cpu-write", "address":row.address.0, "physical":row.address.0, "size":4,
        "value":value.0, "before":before.0.to_le_bytes(), "byte_order":"little", "core":core,
        "pc":if reverse { ROLLBACK_WRITER } else { INSTALL_WRITER }, "instructions":100, "tick":100})
}
fn publish(monitor: &mut LifetimeMonitor, memory: &mut Memory, core: usize, reverse: bool) {
    for index in 0..monitor.config().publications().len() {
        let slot = if reverse { monitor.config().publications().len() - 1 - index } else { index };
        let publication = monitor.config().publications()[slot];
        prepare(monitor, core, publication, reverse).unwrap();
        monitor.observe_row(&publication_row(publication, core, reverse)).unwrap();
        memory.store(publication.address.0, &(if reverse { publication.old } else { publication.new }).0.to_le_bytes());
    }
}
fn cpu_write(monitor: &mut LifetimeMonitor, address: u32, before: &[u8], value: u64) -> Result<(), LifetimeFailure> {
    monitor.observe_write(WriteAttempt::cpu(CoreId(0), Guest(0x1000), Guest(address), Guest(address), before, value))
}

#[test]
fn full_finite_lifetime_requires_actual_entries_and_old_word_verification() {
    let (mut monitor, mut memory) = fixture();
    assert_eq!(monitor.receipt().phase, LifetimePhase::Prepared);
    entry(&mut monitor, 1, false).unwrap();
    publish(&mut monitor, &mut memory, 1, false);
    assert_eq!(monitor.receipt().phase, LifetimePhase::Installed);
    entry(&mut monitor, 0, true).unwrap();
    publish(&mut monitor, &mut memory, 0, true);
    assert_eq!(monitor.receipt().phase, LifetimePhase::RollbackStored);
    monitor.verify_rollback(&memory, 0).unwrap();
    assert_eq!(monitor.receipt().phase, LifetimePhase::RollbackVerified);
    monitor.end().unwrap();
    let receipt = monitor.receipt();
    assert_eq!(receipt.phase, LifetimePhase::Ended);
    assert_eq!(receipt.forward_writes, 223);
    assert_eq!(receipt.reverse_writes, 223);
    assert_eq!(receipt.install_core, Some(1));
    assert_eq!(receipt.rollback_core, Some(0));
    assert_eq!(receipt.pending_stores, 0);
    assert_eq!(receipt.failure, None);
}

#[test]
fn resident_and_settings_identity_stores_preserve_the_initial_snapshot() {
    let (mut monitor, _) = fixture();
    cpu_write(&mut monitor, RESIDENT, &[0; 8], 0).unwrap();
    cpu_write(&mut monitor, SETTINGS + 448, &[0x5a; 2], 0x5a5a).unwrap();
    assert_eq!(monitor.receipt().checked_resource_writes, 2);
    assert_eq!(monitor.receipt().failure, None);
}

#[test]
fn transient_resource_change_cannot_be_repaired_by_a_later_store() {
    for (address, before, changed) in [(RESIDENT, 0, 1), (SETTINGS, 0x5a, 0x5b)] {
        let (mut monitor, _) = fixture();
        assert_eq!(cpu_write(&mut monitor, address, &[before], changed), Err(LifetimeFailure::ResourceChanged));
        assert_eq!(cpu_write(&mut monitor, address, &[changed as u8], u64::from(before)), Err(LifetimeFailure::ResourceChanged));
        assert_eq!(monitor.receipt().failure, Some(LifetimeFailure::ResourceChanged));
    }
}

const SETTER: u32 = 0x0062_ff54;

fn settings_fixture() -> LifetimeMonitor {
    let mut value = request();
    value["settings_writers"] = json!([[0x0062_4694, 0x0063_09c4]]);
    let config = config(value).unwrap();
    let mut memory = Memory::default();
    for (extent, bytes) in config.readonly_images() { memory.store(extent.start().0, bytes); }
    memory.store(SETTINGS, &[0x5a; 450]);
    LifetimeMonitor::new(config, &memory).unwrap()
}
fn write_from(monitor: &mut LifetimeMonitor, pc: u32, address: u32, before: &[u8], value: u64) -> Result<(), LifetimeFailure> {
    monitor.observe_write(WriteAttempt::cpu(CoreId(0), Guest(pc), Guest(address), Guest(address), before, value))
}

#[test]
fn settings_writer_changes_are_admitted_counted_and_become_the_expected_bytes() {
    let mut monitor = settings_fixture();
    write_from(&mut monitor, SETTER, SETTINGS + 0x66, &[0x5a], 0x5b).unwrap();
    assert_eq!(monitor.receipt().admitted_settings_writes, 1);
    write_from(&mut monitor, 0x1000, SETTINGS + 0x66, &[0x5b], 0x5b).unwrap();
    assert_eq!(monitor.receipt().admitted_settings_writes, 1);
    assert_eq!(write_from(&mut monitor, 0x1000, SETTINGS + 0x66, &[0x5a], 0x5a), Err(LifetimeFailure::ResourceChanged));
}

#[test]
fn settings_changes_outside_a_settings_writer_or_the_block_refuse() {
    for (pc, address, before, value) in [
        (0x0062_4690, SETTINGS + 0x66, vec![0x5a], 0x5b),
        (0x0063_09c4, SETTINGS + 0x66, vec![0x5a], 0x5b),
        (SETTER, RESIDENT, vec![0], 1),
    ] {
        let mut monitor = settings_fixture();
        assert_eq!(write_from(&mut monitor, pc, address, &before, value), Err(LifetimeFailure::ResourceChanged), "{pc:#x} {address:#x}");
        assert_eq!(monitor.receipt().admitted_settings_writes, 0);
    }
    let (mut monitor, _) = fixture();
    assert_eq!(write_from(&mut monitor, SETTER, SETTINGS + 0x66, &[0x5a], 0x5b), Err(LifetimeFailure::ResourceChanged));
}

const RELOAD_CALL: u32 = 0x0063_09ec;
const RELOAD_RETURN: u32 = 0x0063_09f0;
const COPY_STORE: u32 = 0x0072_2134;
const RELOAD_START: u32 = 0x0096_cc74;
const RELOAD_END: u32 = 0x0096_cd8c;
const RESTORE_CALL: u32 = 0x0063_7ee4;
const RESTORE_RETURN: u32 = 0x0063_7ee8;

fn stock_reload() -> Value {
    json!({"call": RELOAD_CALL, "return": RELOAD_RETURN, "copy_store": COPY_STORE, "target": [RELOAD_START, RELOAD_END]})
}
fn stock_restore() -> Value {
    json!({"call": RESTORE_CALL, "return": RESTORE_RETURN, "copy_store": COPY_STORE, "target": [SETTINGS, SETTINGS + 450]})
}
fn reload_request(reloads: Value) -> Value {
    let mut value = request();
    value["settings_writers"] = json!([[0x0062_4694, 0x0063_09c4]]);
    value["settings_reloads"] = reloads;
    value
}
fn reload_fixture() -> LifetimeMonitor {
    let config = config(reload_request(json!([stock_reload(), stock_restore()]))).unwrap();
    let mut memory = Memory::default();
    for (extent, bytes) in config.readonly_images() { memory.store(extent.start().0, bytes); }
    memory.store(SETTINGS, &[0x5a; 450]);
    LifetimeMonitor::new(config, &memory).unwrap()
}
fn fetch(monitor: &mut LifetimeMonitor, core: usize, physical: u32) -> Result<(), LifetimeFailure> {
    monitor.observe_row(&json!({"source": "cpu-fetch", "address": physical, "physical": physical, "size": 4,
        "value": null, "before": null, "byte_order": "little", "core": core, "pc": physical, "instructions": 1, "tick": 1}))
}
fn store_on(monitor: &mut LifetimeMonitor, core: usize, pc: u32, address: u32, before: &[u8], value: u64) -> Result<(), LifetimeFailure> {
    monitor.observe_write(WriteAttempt::cpu(CoreId(core), Guest(pc), Guest(address), Guest(address), before, value))
}

#[test]
fn reload_copy_stores_inside_the_bracket_are_admitted_counted_and_become_the_expected_bytes() {
    for core in 0..2 {
        let mut monitor = reload_fixture();
        fetch(&mut monitor, core, RELOAD_CALL).unwrap();
        store_on(&mut monitor, core, COPY_STORE, RELOAD_START, &[0x5a, 0x5a], 0xa9c9).unwrap();
        store_on(&mut monitor, core, COPY_STORE, RELOAD_START + 2, &[0x5a, 0x5a], 0x5a5a).unwrap();
        store_on(&mut monitor, core, COPY_STORE, RELOAD_START + 4, &[0x5a, 0x5a], 0x1234).unwrap();
        fetch(&mut monitor, core, RELOAD_RETURN).unwrap();
        let receipt = monitor.receipt();
        assert_eq!((receipt.admitted_reload_writes, receipt.admitted_settings_writes, receipt.failure), (2, 0, None));
        store_on(&mut monitor, core, 0x1000, RELOAD_START, &[0xc9, 0xa9], 0xa9c9).unwrap();
        assert_eq!(store_on(&mut monitor, core, 0x1000, RELOAD_START, &[0x5a, 0x5a], 0x5a5a), Err(LifetimeFailure::ResourceChanged));
    }
}

#[test]
fn reload_copy_stores_outside_their_open_bracket_refuse() {
    #[derive(Debug)]
    enum Case { NoBracket, OtherCore, WrongPc, BeforeTarget, AfterTarget, AfterReturn, ReturnOnly }
    for case in [Case::NoBracket, Case::OtherCore, Case::WrongPc, Case::BeforeTarget, Case::AfterTarget, Case::AfterReturn, Case::ReturnOnly] {
        let mut monitor = reload_fixture();
        let (mut pc, mut address) = (COPY_STORE, RELOAD_START);
        match case {
            Case::NoBracket => {}
            Case::OtherCore => fetch(&mut monitor, 1, RELOAD_CALL).unwrap(),
            Case::WrongPc => { fetch(&mut monitor, 0, RELOAD_CALL).unwrap(); pc = COPY_STORE + 4; }
            Case::BeforeTarget => { fetch(&mut monitor, 0, RELOAD_CALL).unwrap(); address = RELOAD_START - 2; }
            Case::AfterTarget => { fetch(&mut monitor, 0, RELOAD_CALL).unwrap(); address = RELOAD_END - 1; }
            Case::AfterReturn => { fetch(&mut monitor, 0, RELOAD_CALL).unwrap(); fetch(&mut monitor, 0, RELOAD_RETURN).unwrap(); }
            Case::ReturnOnly => fetch(&mut monitor, 0, RELOAD_RETURN).unwrap(),
        }
        assert_eq!(store_on(&mut monitor, 0, pc, address, &[0x5a, 0x5a], 0xa9c9), Err(LifetimeFailure::ResourceChanged), "{case:?}");
        assert_eq!((monitor.receipt().admitted_reload_writes, monitor.receipt().admitted_settings_writes), (0, 0), "{case:?}");
    }
}

#[test]
fn a_copy_store_off_the_halfword_cursor_closes_the_bracket_and_later_copy_stores_refuse() {
    #[derive(Debug)]
    enum Case { OutOfOrder, Word, SkippedHalfword }
    for case in [Case::OutOfOrder, Case::Word, Case::SkippedHalfword] {
        let mut monitor = reload_fixture();
        fetch(&mut monitor, 0, RELOAD_CALL).unwrap();
        let next = match case {
            Case::OutOfOrder => { store_on(&mut monitor, 0, COPY_STORE, RELOAD_START + 2, &[0x5a, 0x5a], 0x5a5a).unwrap(); RELOAD_START }
            Case::Word => { store_on(&mut monitor, 0, COPY_STORE, RELOAD_START, &[0x5a; 4], 0x5a5a_5a5a).unwrap(); RELOAD_START }
            Case::SkippedHalfword => {
                store_on(&mut monitor, 0, COPY_STORE, RELOAD_START, &[0x5a, 0x5a], 0x5a5a).unwrap();
                store_on(&mut monitor, 0, COPY_STORE, RELOAD_START + 4, &[0x5a, 0x5a], 0x5a5a).unwrap();
                RELOAD_START + 2
            }
        };
        assert_eq!(store_on(&mut monitor, 0, COPY_STORE, next, &[0x5a, 0x5a], 0xa9c9), Err(LifetimeFailure::ResourceChanged), "{case:?}");
        assert_eq!(monitor.receipt().admitted_reload_writes, 0, "{case:?}");
    }
}

#[test]
fn the_back_restore_copies_the_whole_block_in_halfword_order_from_its_own_call() {
    let mut monitor = reload_fixture();
    fetch(&mut monitor, 1, RESTORE_CALL).unwrap();
    for address in (SETTINGS..SETTINGS + 450).step_by(2) {
        let value = if address == 0x0096_ccd2 { 0xa9c9 } else { 0x5a5a };
        store_on(&mut monitor, 1, COPY_STORE, address, &[0x5a, 0x5a], value).unwrap();
    }
    fetch(&mut monitor, 1, RESTORE_RETURN).unwrap();
    assert_eq!((monitor.receipt().admitted_reload_writes, monitor.receipt().failure), (1, None));
    let mut monitor = reload_fixture();
    fetch(&mut monitor, 1, RELOAD_CALL).unwrap();
    assert_eq!(store_on(&mut monitor, 1, COPY_STORE, SETTINGS, &[0x5a, 0x5a], 0xa9c9), Err(LifetimeFailure::ResourceChanged));
}

#[test]
fn interleaved_reload_and_restore_brackets_only_refuse() {
    let mut monitor = reload_fixture();
    fetch(&mut monitor, 0, RESTORE_CALL).unwrap();
    store_on(&mut monitor, 0, COPY_STORE, SETTINGS, &[0x5a, 0x5a], 0x5a5a).unwrap();
    fetch(&mut monitor, 0, RELOAD_RETURN).unwrap();
    assert_eq!(store_on(&mut monitor, 0, COPY_STORE, SETTINGS + 2, &[0x5a, 0x5a], 0xa9c9), Err(LifetimeFailure::ResourceChanged));
    let mut monitor = reload_fixture();
    fetch(&mut monitor, 0, RESTORE_CALL).unwrap();
    for address in (SETTINGS..RELOAD_START).step_by(2) {
        store_on(&mut monitor, 0, COPY_STORE, address, &[0x5a, 0x5a], 0x5a5a).unwrap();
    }
    fetch(&mut monitor, 0, RELOAD_CALL).unwrap();
    fetch(&mut monitor, 0, RELOAD_RETURN).unwrap();
    assert_eq!(store_on(&mut monitor, 0, COPY_STORE, RELOAD_START, &[0x5a, 0x5a], 0xa9c9), Err(LifetimeFailure::ResourceChanged));
    assert_eq!(monitor.receipt().admitted_reload_writes, 0);
}

#[test]
fn reload_site_fetches_are_accepted_in_every_phase_and_require_a_valid_core_and_word() {
    let mut monitor = reload_fixture();
    entry(&mut monitor, 0, false).unwrap();
    fetch(&mut monitor, 1, RELOAD_CALL).unwrap();
    store_on(&mut monitor, 1, COPY_STORE, RELOAD_START, &[0x5a, 0x5a], 0xa9c9).unwrap();
    assert_eq!(monitor.receipt().admitted_reload_writes, 1);
    let mut monitor = reload_fixture();
    assert_eq!(fetch(&mut monitor, 2, RELOAD_CALL), Err(LifetimeFailure::InvalidCore));
    let mut monitor = reload_fixture();
    let row = json!({"source": "cpu-fetch", "address": RELOAD_CALL, "physical": RELOAD_CALL, "size": 2, "core": 0, "pc": RELOAD_CALL, "instructions": 1});
    assert_eq!(monitor.observe_row(&row), Err(LifetimeFailure::MalformedRow));
    let mut monitor = reload_fixture();
    assert_eq!(fetch(&mut monitor, 0, RELOAD_CALL + 4 * 2), Err(LifetimeFailure::MalformedRow));
}

#[test]
fn settings_reloads_are_aligned_bounded_inside_the_block_and_outside_watched_code() {
    let accepted = config(reload_request(json!([stock_reload(), stock_restore()]))).unwrap();
    assert_eq!(accepted.settings_reloads().len(), 2);
    assert!(config(request()).unwrap().settings_reloads().is_empty());
    let with = |key: &str, value: Value| { let mut reload = stock_reload(); reload[key] = value; reload };
    for reloads in [
        json!([with("call", json!(RELOAD_CALL + 2))]),
        json!([with("return", json!(RELOAD_RETURN + 1))]),
        json!([with("copy_store", json!(COPY_STORE + 2))]),
        json!([with("return", json!(RELOAD_CALL))]),
        json!([with("target", json!([SETTINGS - 2, RELOAD_END]))]),
        json!([with("target", json!([RELOAD_START, SETTINGS + 452]))]),
        json!([with("target", json!([RELOAD_START, RELOAD_START]))]),
        json!([with("call", json!(0x00cf_59e4))]),
        json!([with("return", json!(INSTALL + 0x100))]),
        json!([with("copy_store", json!(0x0062_ff54))]),
        json!([stock_reload(), stock_reload()]),
        Value::Array(vec![stock_reload(), { let mut restore = stock_restore(); restore["return"] = json!(RELOAD_CALL); restore }]),
        json!((0..5u32).map(|index| with("call", json!(0x0050_0000 + index * 8))).collect::<Vec<_>>()),
    ] {
        assert!(config(reload_request(reloads.clone())).is_err(), "{reloads}");
    }
    let mut retired = reload_request(json!([stock_reload()]));
    retired["retired"] = json!([[RELOAD_CALL, RELOAD_CALL + 4]]);
    assert!(matches!(config(retired), Err(LifetimeFailure::InvalidSource)));
}

const TEXT_CALL: u32 = 0x0061_c974;
const TEXT_RETURN: u32 = 0x0061_c978;
const FILL_STORE: u32 = 0x0072_21b0;
const TEXT_FIELD: u32 = 0x0096_cd7c;

fn stock_text_setter() -> Value {
    json!({"call": TEXT_CALL, "return": TEXT_RETURN,
        "writers": [[0x0061_dbb4, 0x0061_ddbc], [0x0072_1b40, 0x0072_1bac], [0x0072_2054, 0x0072_2178], [0x0072_21a0, 0x0072_2218]],
        "target": [RELOAD_START, RELOAD_END]})
}
fn call_request(calls: Value) -> Value {
    let mut value = reload_request(json!([stock_reload(), stock_restore()]));
    value["settings_calls"] = calls;
    value
}
fn call_fixture() -> LifetimeMonitor {
    let config = config(call_request(json!([stock_text_setter()]))).unwrap();
    let mut memory = Memory::default();
    for (extent, bytes) in config.readonly_images() { memory.store(extent.start().0, bytes); }
    memory.store(SETTINGS, &[0x5a; 450]);
    LifetimeMonitor::new(config, &memory).unwrap()
}

#[test]
fn a_text_setter_call_admits_its_body_reload_clear_and_copy_and_counts_the_changes() {
    for core in 0..2 {
        let mut monitor = call_fixture();
        fetch(&mut monitor, core, TEXT_CALL).unwrap();
        for address in (RELOAD_START..RELOAD_END).step_by(2) {
            store_on(&mut monitor, core, COPY_STORE, address, &[0x5a, 0x5a], 0x5a5a).unwrap();
        }
        store_on(&mut monitor, core, FILL_STORE, TEXT_FIELD, &[0x5a; 4], 0).unwrap();
        store_on(&mut monitor, core, FILL_STORE, TEXT_FIELD + 4, &[0x5a; 4], 0).unwrap();
        for (offset, value) in [(0, 0x3130), (2, 0x3030), (4, 0x3030), (6, 0x3030)] {
            store_on(&mut monitor, core, COPY_STORE, TEXT_FIELD + offset, &[0, 0], value).unwrap();
        }
        fetch(&mut monitor, core, TEXT_RETURN).unwrap();
        let receipt = monitor.receipt();
        assert_eq!((receipt.admitted_call_writes, receipt.admitted_reload_writes, receipt.admitted_settings_writes, receipt.failure), (6, 0, 0, None));
        store_on(&mut monitor, core, 0x1000, TEXT_FIELD, &[0x30, 0x31], 0x3130).unwrap();
        assert_eq!(store_on(&mut monitor, core, FILL_STORE, TEXT_FIELD, &[0x30, 0x31, 0x30, 0x30], 0), Err(LifetimeFailure::ResourceChanged));
    }
}

#[test]
fn text_setter_stores_outside_their_open_call_refuse() {
    #[derive(Debug)]
    enum Case { NoBracket, OtherCore, WriterEnd, BeforeTarget, AtTargetEnd, StraddlesTarget, AfterReturn, ReturnOnly, ReloadBracket }
    for case in [Case::NoBracket, Case::OtherCore, Case::WriterEnd, Case::BeforeTarget, Case::AtTargetEnd, Case::StraddlesTarget,
        Case::AfterReturn, Case::ReturnOnly, Case::ReloadBracket] {
        let mut monitor = call_fixture();
        let (mut pc, mut address) = (FILL_STORE, TEXT_FIELD);
        match case {
            Case::NoBracket => {}
            Case::OtherCore => fetch(&mut monitor, 1, TEXT_CALL).unwrap(),
            Case::WriterEnd => { fetch(&mut monitor, 0, TEXT_CALL).unwrap(); pc = 0x0072_2218; }
            Case::BeforeTarget => { fetch(&mut monitor, 0, TEXT_CALL).unwrap(); address = RELOAD_START - 4; }
            Case::AtTargetEnd => { fetch(&mut monitor, 0, TEXT_CALL).unwrap(); address = RELOAD_END; }
            Case::StraddlesTarget => { fetch(&mut monitor, 0, TEXT_CALL).unwrap(); address = RELOAD_END - 2; }
            Case::AfterReturn => { fetch(&mut monitor, 0, TEXT_CALL).unwrap(); fetch(&mut monitor, 0, TEXT_RETURN).unwrap(); }
            Case::ReturnOnly => fetch(&mut monitor, 0, TEXT_RETURN).unwrap(),
            Case::ReloadBracket => fetch(&mut monitor, 0, RELOAD_CALL).unwrap(),
        }
        assert_eq!(store_on(&mut monitor, 0, pc, address, &[0x5a; 4], 0x3030_3130), Err(LifetimeFailure::ResourceChanged), "{case:?}");
        let receipt = monitor.receipt();
        assert_eq!((receipt.admitted_call_writes, receipt.admitted_reload_writes, receipt.admitted_settings_writes), (0, 0, 0), "{case:?}");
    }
}

#[test]
fn a_text_setter_call_does_not_open_or_close_a_reload_bracket() {
    let mut monitor = call_fixture();
    fetch(&mut monitor, 0, RELOAD_CALL).unwrap();
    fetch(&mut monitor, 0, TEXT_CALL).unwrap();
    fetch(&mut monitor, 0, TEXT_RETURN).unwrap();
    store_on(&mut monitor, 0, COPY_STORE, RELOAD_START, &[0x5a, 0x5a], 0xa9c9).unwrap();
    assert_eq!((monitor.receipt().admitted_reload_writes, monitor.receipt().admitted_call_writes), (1, 0));
}

#[test]
fn settings_calls_are_aligned_bounded_and_their_sites_and_writers_stay_outside_watched_and_setter_code() {
    let accepted = config(call_request(json!([stock_text_setter()]))).unwrap();
    assert_eq!(accepted.settings_calls().len(), 1);
    assert_eq!(accepted.settings_calls()[0].writers.len(), 4);
    assert!(config(request()).unwrap().settings_calls().is_empty());
    let with = |key: &str, value: Value| { let mut call = stock_text_setter(); call[key] = value; call };
    for calls in [
        json!([with("call", json!(TEXT_CALL + 2))]),
        json!([with("return", json!(TEXT_CALL))]),
        json!([with("writers", json!([]))]),
        json!([with("writers", json!([[0x0072_2054, 0x0072_2176]]))]),
        json!([with("writers", json!([[0x0072_2054, 0x0072_2178], [0x0072_2174, 0x0072_2218]]))]),
        json!([with("writers", json!([[0x0062_ff00, 0x0063_0000]]))]),
        json!([with("writers", json!([[INSTALL, INSTALL + 4]]))]),
        json!([with("writers", json!((0..9u32).map(|index| [0x0070_0000 + index * 8, 0x0070_0004 + index * 8]).collect::<Vec<_>>()))]),
        json!([with("target", json!([RELOAD_START, SETTINGS + 452]))]),
        json!([with("call", json!(RELOAD_CALL))]),
        json!([with("return", json!(0x00cf_59e4))]),
        json!([with("call", json!(INSTALL + 0x100))]),
        json!([stock_text_setter(), stock_text_setter()]),
        Value::Array(vec![{ let mut call = stock_text_setter(); call["copy_store"] = json!(COPY_STORE); call }]),
        json!((0..5u32).map(|index| with("call", json!(0x0050_0000 + index * 8))).collect::<Vec<_>>()),
    ] {
        let refused = serde_json::from_value::<LifetimeRequest>(call_request(calls.clone())).map_or(true, |request| request.into_config(vec![
            PhysicalExtent::new(Guest(0x40000), 0xdae0).unwrap(),
            PhysicalExtent::new(Guest(0xd0000), 0xdaa0).unwrap(),
        ]).is_err());
        assert!(refused, "{calls}");
    }
    let mut retired = call_request(json!([stock_text_setter()]));
    retired["retired"] = json!([[TEXT_RETURN, TEXT_RETURN + 4]]);
    assert!(matches!(config(retired), Err(LifetimeFailure::InvalidSource)));
}

#[test]
fn immutable_roots_width_tables_and_code_refuse_even_identity_stores() {
    for address in [0x00cf_e3d4, 0x00d0_0268, 0x00cd_5f0e, 0x00cd_1ae4,
        0x00d0_4080, 0x00d0_1370, 0x40000, 0xd0000, INSTALL] {
        let (mut monitor, _) = fixture();
        assert_eq!(cpu_write(&mut monitor, address, &[0], 0), Err(LifetimeFailure::ImmutableWrite));
    }
}

#[test]
fn host_and_model_writes_are_refused() {
    for source in [WriteSource::Host, WriteSource::Model] {
        let (mut monitor, _) = fixture();
        assert_eq!(monitor.observe_write(WriteAttempt::cpu(CoreId(0), Guest(INSTALL), Guest(RESIDENT), Guest(RESIDENT), &[0], 0)
            .with_source(source)), Err(LifetimeFailure::NonCpuWrite));
    }
}

#[test]
fn creation_and_backlink_identity_reuse_refuses_but_scheduler_fields_remain_mutable() {
    for layout in source_layouts() {
        let (mut monitor, _) = fixture();
        cpu_write(&mut monitor, layout.nodes.start().0, &[0; 4], 7).unwrap();
        cpu_write(&mut monitor, layout.nodes.start().0 + 12, &[0; 4], 9).unwrap();
        assert_eq!(monitor.receipt().checked_scheduler_writes, 2);
        for address in [layout.creations.start().0, layout.creations.end().0 - 1,
            layout.nodes.start().0 + 8, layout.nodes.end().0 - 56 + 8] {
            let (mut monitor, _) = fixture();
            assert_eq!(cpu_write(&mut monitor, address, &[0], 0), Err(LifetimeFailure::IdentityWrite));
            assert_eq!(monitor.receipt().checked_identity_writes, 1);
        }
        let (mut monitor, _) = fixture();
        assert_eq!(cpu_write(&mut monitor, layout.nodes.start().0 + 7, &[0; 2], 0), Err(LifetimeFailure::IdentityWrite));
    }
}

#[test]
fn missing_entry_or_writer_permit_never_admits_a_publication() {
    let (mut monitor, _) = fixture();
    let row = monitor.config().publications()[0];
    assert_eq!(prepare(&mut monitor, 0, row, false), Err(LifetimeFailure::WrongPhase));
    let (mut monitor, _) = fixture();
    entry(&mut monitor, 0, false).unwrap();
    assert_eq!(monitor.observe_row(&publication_row(row, 0, false)), Err(LifetimeFailure::MissingPermit));
}

#[test]
fn publication_writer_validates_order_core_prior_word_registers_and_instruction() {
    for case in 0..6 {
        let (mut monitor, _) = fixture();
        entry(&mut monitor, 0, false).unwrap();
        let first = monitor.config().publications()[0];
        let second = monitor.config().publications()[1];
        let mut registers = [0; 13];
        registers[1] = first.address.0;
        registers[3] = first.new.0;
        let mut core = CoreId(0);
        let mut before = first.old;
        let mut pc = Guest(INSTALL_WRITER);
        let mut word = Word(STR);
        let expected = match case {
            0 => { registers[1] = second.address.0; LifetimeFailure::WrongOrder },
            1 => { core = CoreId(1); LifetimeFailure::WrongCore },
            2 => { before = Word(44); LifetimeFailure::WrongBefore },
            3 => { registers[3] = first.old.0; LifetimeFailure::WrongValue },
            4 => { pc = Guest(ROLLBACK_WRITER); LifetimeFailure::WrongWriter },
            _ => { word = Word(STR ^ 1); LifetimeFailure::WrongWriter },
        };
        assert_eq!(monitor.prepare_store(core, pc, word, &registers, before, Cpsr(0x13)), Err(expected));
    }
}

#[test]
fn publication_permits_are_single_use_and_cannot_be_overwritten() {
    let (mut monitor, _) = fixture();
    entry(&mut monitor, 0, false).unwrap();
    let row = monitor.config().publications()[0];
    prepare(&mut monitor, 0, row, false).unwrap();
    assert_eq!(monitor.receipt().pending_stores, 1);
    assert_eq!(prepare(&mut monitor, 0, row, false), Err(LifetimeFailure::PendingStore));
    let (mut monitor, _) = fixture();
    entry(&mut monitor, 0, false).unwrap();
    prepare(&mut monitor, 0, row, false).unwrap();
    monitor.observe_row(&publication_row(row, 0, false)).unwrap();
    assert_eq!(monitor.observe_row(&publication_row(row, 0, false)), Err(LifetimeFailure::MissingPermit));
}

#[test]
fn every_publication_row_field_is_bound_to_its_preexecution_permit() {
    for (field, changed, expected) in [
        ("pc", json!(INSTALL), LifetimeFailure::WrongWriter),
        ("core", json!(1), LifetimeFailure::MissingPermit),
        ("address", json!(0x4000_0000u32 + 0x00cf_59e4), LifetimeFailure::WrongAddress),
        ("physical", json!(0x00cf_59e8), LifetimeFailure::WrongAddress),
        ("size", json!(2), LifetimeFailure::InvalidWrite),
        ("before", json!([1,0,0,0]), LifetimeFailure::WrongBefore),
        ("value", json!(0), LifetimeFailure::WrongValue),
        ("byte_order", json!("big"), LifetimeFailure::MalformedRow),
        ("source", json!("host-write"), LifetimeFailure::NonCpuWrite),
        ("tcm", json!({"bank":"atcm","offset":0}), LifetimeFailure::MalformedRow),
    ] {
        let (mut monitor, _) = fixture();
        entry(&mut monitor, 0, false).unwrap();
        let publication = monitor.config().publications()[0];
        prepare(&mut monitor, 0, publication, false).unwrap();
        let mut row = publication_row(publication, 0, false);
        row[field] = changed;
        assert_eq!(monitor.observe_row(&row), Err(expected));
        assert_eq!(monitor.receipt().forward_writes, 0);
        assert_eq!(monitor.receipt().pending_stores, 1);
    }
}

#[test]
fn partial_publications_never_consume_a_permit() {
    let (mut monitor, _) = fixture();
    entry(&mut monitor, 0, false).unwrap();
    let row = monitor.config().publications()[0];
    prepare(&mut monitor, 0, row, false).unwrap();
    assert_eq!(monitor.observe_write(WriteAttempt::cpu(CoreId(0), Guest(INSTALL_WRITER), row.address, row.address, &[0; 2], 1)),
        Err(LifetimeFailure::WrongWidth));
    assert_eq!(monitor.receipt().pending_stores, 1);
}

#[test]
fn duplicate_missing_and_premature_lifecycle_stages_refuse() {
    let (mut monitor, _) = fixture();
    assert_eq!(entry(&mut monitor, 0, true), Err(LifetimeFailure::WrongPhase));
    let (mut monitor, _) = fixture();
    entry(&mut monitor, 0, false).unwrap();
    assert_eq!(entry(&mut monitor, 0, false), Err(LifetimeFailure::WrongPhase));
    let (mut monitor, _) = fixture();
    entry(&mut monitor, 0, false).unwrap();
    assert_eq!(entry(&mut monitor, 0, true), Err(LifetimeFailure::WrongPhase));
    let (mut monitor, memory) = fixture();
    assert_eq!(monitor.verify_rollback(&memory, 0), Err(LifetimeFailure::WrongPhase));
    let (mut monitor, _) = fixture();
    assert_eq!(monitor.end(), Err(LifetimeFailure::WrongPhase));
}

#[test]
fn reverse_publications_require_reverse_manifest_order() {
    let (mut monitor, mut memory) = fixture();
    entry(&mut monitor, 0, false).unwrap();
    publish(&mut monitor, &mut memory, 0, false);
    entry(&mut monitor, 1, true).unwrap();
    let first = monitor.config().publications()[0];
    assert_eq!(prepare(&mut monitor, 1, first, true), Err(LifetimeFailure::WrongOrder));
}

#[test]
fn rollback_verification_checks_memory_and_active_modal_frames() {
    for active_frames in [0, 1] {
        let (mut monitor, mut memory) = fixture();
        entry(&mut monitor, 0, false).unwrap();
        publish(&mut monitor, &mut memory, 0, false);
        entry(&mut monitor, 0, true).unwrap();
        publish(&mut monitor, &mut memory, 0, true);
        if active_frames == 0 { memory.store(monitor.config().publications()[0].address.0, &[9]); }
        assert_eq!(monitor.verify_rollback(&memory, active_frames), Err(if active_frames == 0 {
            LifetimeFailure::IncompleteRollback
        } else { LifetimeFailure::ActiveFrames }));
        assert_eq!(monitor.receipt().phase, LifetimePhase::RollbackStored);
    }
}

#[test]
fn fixed_watch_extents_are_sorted_merged_and_cover_both_task_kernels() {
    let config = config(request()).unwrap();
    assert!(config.watched_extents().windows(2).all(|pair| pair[0].end() < pair[1].start()));
    for layout in source_layouts() {
        for target in [layout.context, layout.current, layout.selected, layout.nodes, layout.creations] {
            assert!(config.watched_extents().iter().any(|range| range.start() <= target.start() && target.end() <= range.end()));
        }
    }
    assert!(config.code_sites().iter().any(|site| site.pc == Guest(0x436b8) && site.word == Word(0xe582_1000)));
    assert!(config.code_sites().iter().any(|site| site.pc == Guest(0xd0240) && site.word == Word(0xe582_0000)));
}

#[test]
fn request_validation_refuses_aliases_duplicate_addresses_identity_publications_and_wrong_sizes() {
    for case in 0..5 {
        let mut value = request();
        match case {
            0 => value["publications"][0]["address"] = json!(0x40cf_59e4u32),
            1 => value["publications"][1]["address"] = value["publications"][0]["address"].clone(),
            2 => value["publications"][0]["new"] = json!(0),
            3 => value["resident"][1] = json!(RESIDENT + 1023),
            _ => { value["publications"].as_array_mut().unwrap().pop(); },
        }
        assert!(config(value).is_err());
    }
    let mut value = request();
    value["qualification"] = json!(true);
    assert!(serde_json::from_value::<LifetimeRequest>(value).is_err());
}

#[test]
fn retired_extents_are_aligned_disjoint_bounded_and_outside_every_watched_source() {
    let mut accepted = request();
    accepted["retired"] = json!([[0x00c7_a6ac, 0x00c7_a6ec], [0x00c7_985c, 0x00c7_986c]]);
    let accepted = config(accepted).unwrap();
    let starts: Vec<_> = accepted.retired_extents().iter().map(|extent| extent.start().0).collect();
    assert_eq!(starts, vec![0x00c7_985c, 0x00c7_a6ac]);
    for retired in [
        json!([[0x00c7_a6ae, 0x00c7_a6ec]]),
        json!([[0x00c7_a6ac, 0x00c7_a6ac]]),
        json!([[0x00c7_a6ac, 0x00c7_a6ec], [0x00c7_a6e8, 0x00c7_a700]]),
        json!([[INSTALL, INSTALL + 8]]),
        json!([[SETTINGS & !3, (SETTINGS & !3) + 8]]),
        json!([[0x0004_0000, 0x0004_0010]]),
        json!((0..65u32).map(|index| [0x00c0_0000 + index * 8, 0x00c0_0004 + index * 8]).collect::<Vec<_>>()),
    ] {
        let mut value = request();
        value["retired"] = retired;
        assert!(config(value).is_err());
    }
    assert!(config(request()).unwrap().retired_extents().is_empty());
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage { Prepared, Publishing, Installed, RollingBack, RollbackStored }

fn retired_at(stage: Stage) -> LifetimeMonitor {
    let mut value = request();
    value["retired"] = json!([[0x00c7_aef4, 0x00c7_af6c], [0x00c7_af6c, 0x00c7_afc8]]);
    let config = config(value).unwrap();
    let mut memory = Memory::default();
    for (extent, bytes) in config.readonly_images() { memory.store(extent.start().0, bytes); }
    memory.store(SETTINGS, &[0x5a; 450]);
    let mut monitor = LifetimeMonitor::new(config, &memory).unwrap();
    if stage == Stage::Prepared { return monitor; }
    entry(&mut monitor, 0, false).unwrap();
    if stage == Stage::Publishing { return monitor; }
    publish(&mut monitor, &mut memory, 0, false);
    if stage == Stage::Installed { return monitor; }
    entry(&mut monitor, 0, true).unwrap();
    if stage == Stage::RollingBack { return monitor; }
    publish(&mut monitor, &mut memory, 0, true);
    monitor
}

#[test]
fn retired_fetches_refuse_from_install_entry_through_the_last_reverse_write() {
    for physical in [0x00c7_aef4u32, 0x00c7_af68, 0x00c7_af6c, 0x00c7_afc4] {
        let row = json!({"source": "cpu-fetch", "address": physical, "physical": physical, "size": 4, "core": 1, "pc": physical, "instructions": 1});
        for (stage, refused) in [(Stage::Prepared, false), (Stage::Publishing, true), (Stage::Installed, true), (Stage::RollingBack, true), (Stage::RollbackStored, false)] {
            let mut monitor = retired_at(stage);
            let result = monitor.observe_row(&row);
            assert_eq!(result, if refused { Err(LifetimeFailure::RetiredFetch) } else { Ok(()) }, "{stage:?} {physical:#x}");
            assert_eq!(monitor.receipt().failure, result.err());
        }
    }
    let mut monitor = retired_at(Stage::Prepared);
    let outside = json!({"source": "cpu-fetch", "address": 0x00c7_afc8, "physical": 0x00c7_afc8, "size": 4, "core": 0, "pc": 0x00c7_afc8, "instructions": 1});
    assert_eq!(monitor.observe_row(&outside), Err(LifetimeFailure::MalformedRow));
}

#[test]
fn initial_resident_and_readonly_bytes_are_validated() {
    for resident in [true, false] {
        let config = config(request()).unwrap();
        let mut memory = Memory::default();
        for (extent, bytes) in config.readonly_images() { memory.store(extent.start().0, bytes); }
        memory.store(if resident { RESIDENT } else { INSTALL }, &[0xff]);
        assert!(matches!(LifetimeMonitor::new(config, &memory), Err(error) if error == if resident {
            LifetimeFailure::InitialResident
        } else { LifetimeFailure::InitialReadonly }));
    }
}

fn context_store(monitor: &mut LifetimeMonitor, core: usize, pc: u32, before: u32, after: u32) -> Result<(), LifetimeFailure> {
    let layout = source_layouts()[core];
    let mut registers = [0; 13];
    let (word, value_register, target_register) = match pc & 0xffff {
        0x0240 => (0xe582_0000, 0, 2),
        0x031c => (0xe582_3000, 3, 2),
        _ => (0xe586_3000, 3, 6),
    };
    registers[target_register] = layout.context.start().0 + 0x4000_0000;
    registers[value_register] = after;
    monitor.prepare_store(CoreId(core), Guest(pc), Word(word), &registers, Word(before), Cpsr(0x93))?;
    monitor.observe_write(WriteAttempt::cpu(CoreId(core), Guest(pc), Guest(registers[target_register]), layout.context.start(), &before.to_le_bytes(), u64::from(after)))
}

#[test]
fn source_bound_context_transitions_admit_irq_nesting_and_scheduler_stores() {
    for (core, delta) in [(0, 0), (1, 0x90000)] {
        let (mut monitor, _) = fixture();
        for (pc, before, after) in [(0x40240,0,1), (0x40240,1,2), (0x4031c,2,1), (0x4031c,1,0),
            (0x43690,0,0), (0x436f0,0,1), (0x43720,1,0)] {
            context_store(&mut monitor, core, pc + delta, before, after).unwrap();
        }
        assert_eq!(monitor.receipt().checked_context_writes, 7);
    }
}

#[test]
fn fabricated_context_transitions_and_initialization_refuse() {
    for (pc, before, after) in [(0x40240,0,0), (0x4031c,0,0), (0x43690,1,0), (0x436f0,1,1),
        (0x43720,0,0), (0x43848,0,1)] {
        let (mut monitor, _) = fixture();
        assert!(context_store(&mut monitor, 0, pc, before, after).is_err());
    }
    let (mut monitor, _) = fixture();
    assert_eq!(context_store(&mut monitor, 1, 0x40240, 0, 1), Err(LifetimeFailure::WrongCore));
}

#[test]
fn current_task_store_requires_observed_selected_cell_zero_context_and_local_node_namespace() {
    for case in 0..5 {
        let (mut monitor, _) = fixture();
        let layout = source_layouts()[0];
        let node = layout.nodes.start().0 + 0x4000_0000;
        let selected = if case == 1 { 0 } else { node };
        let mut registers = [0; 13];
        registers[1] = if case == 4 { source_layouts()[1].nodes.start().0 + 0x4000_0000 } else { selected };
        registers[2] = layout.current.start().0 + 0x4000_0000;
        let current = CurrentState { selected: Word(if case == 2 { 0 } else { selected }), context_depth: Word(if case == 3 { 1 } else { 0 }) };
        let result = monitor.prepare_current_store(CoreId(0), Guest(0x436b8), Word(0xe582_1000), &registers, Word(node), Cpsr(0x93), current);
        if case >= 2 { assert_eq!(result, Err(LifetimeFailure::InvalidContext)); }
        else {
            result.unwrap();
            monitor.observe_write(WriteAttempt::cpu(CoreId(0), Guest(0x436b8), Guest(registers[2]), layout.current.start(), &node.to_le_bytes(), u64::from(selected))).unwrap();
            assert_eq!(monitor.receipt().checked_context_writes, 1);
        }
    }
}

const MAKE_READY: [u32; 2] = [0x43d08, 0xd3cc4];
fn selected_write(monitor: &mut LifetimeMonitor, kernel: usize, core: usize, pc: u32, before: u32, value: u32) -> Result<(), LifetimeFailure> {
    let cell = source_layouts()[kernel].selected.start();
    monitor.observe_write(WriteAttempt::cpu(CoreId(core), Guest(pc), Guest(cell.0 + 0x4000_0000), cell, &before.to_le_bytes(), u64::from(value)))
}
fn current_store(monitor: &mut LifetimeMonitor, kernel: usize, loaded: u32, selected: u32, before: u32) -> Result<(), LifetimeFailure> {
    let layout = source_layouts()[kernel];
    let pc = Guest([0x436b8, 0xd36b8][kernel]);
    let mut registers = [0; 13];
    registers[1] = loaded;
    registers[2] = layout.current.start().0 + 0x4000_0000;
    monitor.prepare_current_store(CoreId(kernel), pc, Word(0xe582_1000), &registers, Word(before), Cpsr(0x93),
        CurrentState { selected: Word(selected), context_depth: Word(0) })?;
    monitor.observe_write(WriteAttempt::cpu(CoreId(kernel), pc, Guest(registers[2]), layout.current.start(), &before.to_le_bytes(), u64::from(loaded)))
}

#[test]
fn current_store_admits_a_selection_the_other_cores_make_ready_replaced_and_refuses_unexplained_ones() {
    for kernel in [0, 1] {
        let other = 1 - kernel;
        let node = |index: u32| source_layouts()[kernel].nodes.start().0 + 0x4000_0000 + 56 * index;
        let (loaded, woken, previous) = (node(1), node(2), node(3));
        // The other core's make-ready replaced the selection this core had loaded.
        let (mut monitor, _) = fixture();
        selected_write(&mut monitor, kernel, other, MAKE_READY[other], loaded, woken).unwrap();
        current_store(&mut monitor, kernel, loaded, woken, previous).unwrap();
        assert_eq!(monitor.receipt().checked_context_writes, 1);
        // The admission lasts until that store.
        assert_eq!(current_store(&mut monitor, kernel, loaded, woken, loaded), Err(LifetimeFailure::InvalidContext));
        // No rewrite, a rewrite from another PC, and one followed by this core's own write are refused.
        let (mut monitor, _) = fixture();
        assert_eq!(current_store(&mut monitor, kernel, loaded, woken, previous), Err(LifetimeFailure::InvalidContext));
        let (mut monitor, _) = fixture();
        selected_write(&mut monitor, kernel, other, MAKE_READY[other] + 4, loaded, woken).unwrap();
        assert_eq!(current_store(&mut monitor, kernel, loaded, woken, previous), Err(LifetimeFailure::InvalidContext));
        let (mut monitor, _) = fixture();
        selected_write(&mut monitor, kernel, other, MAKE_READY[other], loaded, woken).unwrap();
        selected_write(&mut monitor, kernel, kernel, MAKE_READY[kernel], woken, woken).unwrap();
        assert_eq!(current_store(&mut monitor, kernel, loaded, woken, previous), Err(LifetimeFailure::InvalidContext));
    }
}

#[test]
fn selected_cell_rewrites_beyond_the_priority_levels_and_partial_writes_refuse() {
    let (mut monitor, _) = fixture();
    for _ in 0..17 { selected_write(&mut monitor, 0, 1, MAKE_READY[1], 0, 0).unwrap(); }
    assert_eq!(selected_write(&mut monitor, 0, 1, MAKE_READY[1], 0, 0), Err(LifetimeFailure::InvalidContext));
    let (mut monitor, _) = fixture();
    let cell = source_layouts()[0].selected.start();
    assert_eq!(monitor.observe_write(WriteAttempt::cpu(CoreId(1), Guest(MAKE_READY[1]), Guest(cell.0 + 0x4000_0000), cell, &[0], 1)),
        Err(LifetimeFailure::WrongWidth));
}

#[test]
fn publication_on_a_selected_cell_is_an_invalid_request() {
    for layout in source_layouts() {
        let mut value = request();
        value["publications"][3]["address"] = json!(layout.selected.start().0);
        assert_eq!(config(value).err(), Some(LifetimeFailure::InvalidRequest));
    }
}

#[test]
fn malformed_or_unwatched_rows_do_not_disappear() {
    for row in [json!({}), json!({"source":"cpu-read"})] {
        let (mut monitor, _) = fixture();
        assert_eq!(monitor.observe_row(&row), Err(LifetimeFailure::MalformedRow));
    }
    let (mut monitor, _) = fixture();
    assert_eq!(cpu_write(&mut monitor, 0x1000, &[0], 0), Err(LifetimeFailure::OutsideWatch));
}

#[test]
fn repeated_rollback_entry_end_and_missing_final_reverse_store_cannot_complete() {
    let (mut monitor, mut memory) = fixture();
    entry(&mut monitor, 0, false).unwrap();
    publish(&mut monitor, &mut memory, 0, false);
    entry(&mut monitor, 0, true).unwrap();
    assert_eq!(entry(&mut monitor, 0, true), Err(LifetimeFailure::WrongPhase));
    let (mut monitor, mut memory) = fixture();
    entry(&mut monitor, 0, false).unwrap();
    publish(&mut monitor, &mut memory, 0, false);
    entry(&mut monitor, 0, true).unwrap();
    for slot in (1..223).rev() {
        let row = monitor.config().publications()[slot];
        prepare(&mut monitor, 0, row, true).unwrap();
        monitor.observe_row(&publication_row(row, 0, true)).unwrap();
        memory.store(row.address.0, &row.old.0.to_le_bytes());
    }
    assert_eq!(monitor.verify_rollback(&memory, 0), Err(LifetimeFailure::WrongPhase));
    assert_eq!(monitor.receipt().reverse_writes, 222);
    let (mut monitor, mut memory) = fixture();
    entry(&mut monitor, 0, false).unwrap();
    publish(&mut monitor, &mut memory, 0, false);
    entry(&mut monitor, 0, true).unwrap();
    publish(&mut monitor, &mut memory, 0, true);
    monitor.verify_rollback(&memory, 0).unwrap();
    monitor.end().unwrap();
    assert_eq!(monitor.end(), Err(LifetimeFailure::WrongPhase));
}

#[test]
fn publication_identity_store_and_reused_prior_allowance_are_refused() {
    let (mut monitor, _) = fixture();
    entry(&mut monitor, 0, false).unwrap();
    let row = monitor.config().publications()[0];
    prepare(&mut monitor, 0, row, false).unwrap();
    let mut identity = publication_row(row, 0, false);
    identity["value"] = json!(row.old.0);
    assert_eq!(monitor.observe_row(&identity), Err(LifetimeFailure::WrongValue));
    let (mut monitor, _) = fixture();
    entry(&mut monitor, 0, false).unwrap();
    prepare(&mut monitor, 0, row, false).unwrap();
    monitor.observe_row(&publication_row(row, 0, false)).unwrap();
    assert_eq!(prepare(&mut monitor, 0, row, false), Err(LifetimeFailure::WrongOrder));
}

#[test]
fn context_store_requires_source_word_canonical_target_mask_and_preexecution_permit() {
    for case in 0..4 {
        let (mut monitor, _) = fixture();
        let layout = source_layouts()[0];
        let mut registers = [0; 13];
        registers[0] = 1;
        registers[2] = layout.context.start().0 + 0x4000_0000;
        let mut word = Word(0xe582_0000);
        let mut cpsr = Cpsr(0x93);
        let expected = match case {
            0 => { word = Word(0xe582_0004); LifetimeFailure::WrongWriter },
            1 => { registers[2] = layout.context.start().0; LifetimeFailure::WrongAddress },
            2 => { cpsr = Cpsr(0x13); LifetimeFailure::InvalidContext },
            _ => { cpsr = Cpsr(0x92); LifetimeFailure::InvalidContext },
        };
        assert_eq!(monitor.prepare_store(CoreId(0), Guest(0x40240), word, &registers, Word(0), cpsr), Err(expected));
    }
    let (mut monitor, _) = fixture();
    assert_eq!(cpu_write(&mut monitor, source_layouts()[0].context.start().0, &[0; 4], 1), Err(LifetimeFailure::MissingPermit));
}

#[test]
fn immutable_site_metadata_and_initial_publication_preimages_are_required() {
    let mut value = request();
    value["install"]["writer"]["word"] = json!(STR ^ 1);
    assert!(matches!(config(value), Err(LifetimeFailure::InvalidSource)));
    let parsed: LifetimeRequest = serde_json::from_value(request()).unwrap();
    assert!(matches!(parsed.into_config(vec![PhysicalExtent::new(Guest(0x40000), 4).unwrap()]), Err(LifetimeFailure::InvalidSource)));
    let config = config(request()).unwrap();
    let mut memory = Memory::default();
    for (extent, bytes) in config.readonly_images() { memory.store(extent.start().0, bytes); }
    memory.store(config.publications()[0].address.0, &[1, 0, 0, 0]);
    assert!(matches!(LifetimeMonitor::new(config, &memory), Err(LifetimeFailure::WrongBefore)));
}

#[derive(Default)]
struct Tasks(BTreeMap<u32, u8>);
impl Tasks {
    fn put(&mut self, address: u32, word: u32) {
        self.0.extend(word.to_le_bytes().into_iter().enumerate().map(|(index, byte)| ((address & !0x4000_0000) + index as u32, byte)));
    }
}
impl ValidatedTaskMemory for Tasks {
    fn read(&self, address: Guest, output: &mut [u8]) -> Result<(), MemoryError> {
        for (index, byte) in output.iter_mut().enumerate() {
            *byte = *self.0.get(&((address.0 & !0x4000_0000) + index as u32)).ok_or(MemoryError::Unmapped)?;
        }
        Ok(())
    }
    fn validate_stack(&self, _: StackExtent) -> Result<(), MemoryError> { Ok(()) }
}
fn created_task(core: usize, id: u16) -> TaskId {
    let layout = source_layouts()[core];
    let mut tasks = Tasks::default();
    let creation = layout.creations.start().0 + 40 * u32::from(id - 1);
    tasks.put(layout.nodes.start().0 + 56 * u32::from(id - 1) + 8, creation | 0x4000_0000);
    for offset in (0..40).step_by(4) { tasks.put(creation + offset, 0); }
    for (offset, word) in [(32, u32::from(id)), (36, 1)] { tasks.put(creation + offset, word); }
    match vfp_owner(&tasks, layout.kernel, u32::from(id)) { Ok(VfpOwner::Task(task)) => task, other => panic!("{other:?}") }
}
fn vfp_store(monitor: &mut LifetimeMonitor, core: usize, before: VfpOwner, current: TaskId) -> Result<(), LifetimeFailure> {
    let cell = source_layouts()[core].vfp_owner.start();
    let pc = Guest(if core == 0 { 0x42440 } else { 0xd2440 });
    let mut registers = [0; 13];
    registers[3] = cell.0;
    registers[5] = u32::from(current.get());
    monitor.prepare_vfp_owner_store(CoreId(core), pc, Word(0xe583_5000), &registers, Word(before.word()), Cpsr(0x6000_019b),
        VfpOwnerState { before, current })?;
    monitor.observe_write(WriteAttempt::cpu(CoreId(core), pc, cell, cell, &before.word().to_le_bytes(), u64::from(current.get())))
}

#[test]
fn lazy_vfp_owner_stores_install_the_trapping_current_task_from_its_source_store() {
    for core in 0..2 {
        let (mut monitor, _) = fixture();
        let (first, second) = (created_task(core, 3), created_task(core, 7));
        vfp_store(&mut monitor, core, VfpOwner::Unowned, first).unwrap();
        vfp_store(&mut monitor, core, VfpOwner::Task(first), second).unwrap();
        vfp_store(&mut monitor, core, VfpOwner::Task(second), second).unwrap();
        let receipt = monitor.receipt();
        assert_eq!((receipt.checked_vfp_owner_writes, receipt.checked_context_writes, receipt.failure), (3, 0, None));
    }
}

#[test]
fn lazy_vfp_owner_stores_refuse_other_values_modes_aliases_cores_and_unpermitted_writes() {
    let layout = source_layouts()[0];
    let current = created_task(0, 3);
    let state = VfpOwnerState { before: VfpOwner::Unowned, current };
    for case in 0..6 {
        let (mut monitor, _) = fixture();
        let mut registers = [0; 13];
        registers[3] = layout.vfp_owner.start().0;
        registers[5] = 3;
        let (mut core, mut before, mut cpsr) = (0, Word(VfpOwner::UNOWNED), Cpsr(0x9b));
        let expected = match case {
            0 => { registers[5] = 4; LifetimeFailure::InvalidContext },
            1 => { registers[5] = 0x8000_0003; LifetimeFailure::InvalidContext },
            2 => { cpsr = Cpsr(0x93); LifetimeFailure::InvalidContext },
            3 => { registers[3] |= 0x4000_0000; LifetimeFailure::WrongAddress },
            4 => { core = 1; LifetimeFailure::WrongCore },
            _ => { before = Word(3); LifetimeFailure::InvalidContext },
        };
        assert_eq!(monitor.prepare_vfp_owner_store(CoreId(core), Guest(0x42440), Word(0xe583_5000), &registers, before, cpsr, state), Err(expected));
    }
    let (mut monitor, _) = fixture();
    let mut registers = [0; 13];
    registers[3] = layout.vfp_owner.start().0;
    registers[5] = 3;
    assert_eq!(monitor.prepare_store(CoreId(0), Guest(0x42440), Word(0xe583_5000), &registers, Word(VfpOwner::UNOWNED), Cpsr(0x9b)),
        Err(LifetimeFailure::InvalidContext));
    let (mut monitor, _) = fixture();
    assert_eq!(cpu_write(&mut monitor, layout.vfp_owner.start().0, &VfpOwner::UNOWNED.to_le_bytes(), 3), Err(LifetimeFailure::MissingPermit));
    let (mut monitor, _) = fixture();
    assert_eq!(cpu_write(&mut monitor, layout.vfp_owner.start().0 - 4, &[0; 4], 0), Err(LifetimeFailure::ImmutableWrite));
}
