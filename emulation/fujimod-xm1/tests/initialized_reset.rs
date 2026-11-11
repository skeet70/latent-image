use fujimod_emulator::{
    board::{bus::{Bus, Mode}, factory::{self, Graph, MemoryMap, Processor, Ram}, memory::BusMapping},
    session::Session,
};
use fujimod_emulator_cpu::{ExecutionPolicy, Model, Register, arm, timeline::Ratio};
use fujimod_emulator_exr2_plan::Resources;
use fujimod_xm1::{
    bridge::{CheckpointBudget, SNAPSHOT},
    checkpoint::{BootIdentity, CacheLookup, ResetRestore, ResetCachePolicy, initialized_reset},
    machine::Machine,
    machine_fingerprint::{fingerprint, snapshot_digest},
    run::RunDocument,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::atomic::{AtomicU64, Ordering}};

fn machine() -> Machine {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let graph = Graph {
        processors: (0..2).map(|core| Processor {
            model: Model::CortexR5F, rprofile: Some(Default::default()), mprofile: None,
            released: true,
            registers: vec![(Register::Pc, 0x1000 + core * 0x1000), (Register::Pstate, 0xd3)],
        }).collect(),
        ram: vec![Ram { base: 0, size: 0x4000 }],
        dma_mappings: vec![BusMapping::ram(0, 0x4000)],
        mappings: (0..2).map(|core| MemoryMap {
            core, address: 0, physical: 0, size: 0x4000, permissions: 7,
        }).collect(),
        decoders: vec![], peripherals: vec![],
        instructions_per_tick: Ratio::new(24, 1).unwrap(),
    };
    let mut board = graph.build(factory::Options {
        engine: factory::Engine::Unicorn, translation_cache_bytes: 1 << 20,
    }, Bus::new(Mode::Strict), None).unwrap();
    board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    let session = Session {
        definition: Value::Null, board, controls: None, contact_driver: None,
        resources: Resources::default(), nodes: BTreeMap::new(), contacts: BTreeMap::new(),
        recording_frames: Default::default(),
    };
    Machine::new(RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap(), session).unwrap()
}

struct CacheRoot(PathBuf);
impl CacheRoot {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!("xm1-initialized-reset-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for CacheRoot {
    fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); }
}

fn identity() -> BootIdentity {
    BootIdentity::new(b"synthetic-run", b"synthetic-dat", b"synthetic-rom", b"synthetic-nand", None, b"zero-instruction-reset-test")
}

fn proof(machine: &Machine) -> Value {
    assert_eq!(machine.session.board.instruction_counts(), vec![0, 0]);
    assert_eq!(machine.session.board.now(), 0);
    let proof = fingerprint(&machine.session).unwrap();
    assert_eq!(proof["status"], "complete");
    assert_eq!(proof["counts"], json!([0, 0]));
    proof
}

#[test]
fn miss_preserves_live_state_and_hit_restores_saved_state_without_instructions() {
    let root = CacheRoot::new();
    let identity = identity();
    let original = machine();
    let before_miss = proof(&original);
    let miss = initialized_reset(original, &identity, &root.0, None, ResetCachePolicy::ReuseOrCreate).unwrap();
    assert_eq!(miss.cache.lookup(), CacheLookup::Miss);
    assert_eq!(miss.cache.restore(), ResetRestore::NotRestored);
    assert_eq!(serde_json::to_value(miss.cache).unwrap(), json!({"lookup":"miss", "restore":"not-restored"}));
    assert_eq!(proof(&miss.machine), before_miss);

    let directory = root.0.join(format!("reset-{}", identity.key()));
    let saved_digest = snapshot_digest(&directory.join(SNAPSHOT)).unwrap();
    let saved_bytes = std::fs::read(directory.join(SNAPSHOT)).unwrap();
    let mut independently_restored = machine();
    let snapshot = independently_restored.session.import_snapshot(&saved_bytes).unwrap();
    independently_restored.disarm().unwrap();
    independently_restored.session.restore(&snapshot).unwrap();
    independently_restored.rearm().unwrap();
    let restored_proof = proof(&independently_restored);
    assert_eq!(restored_proof, before_miss);

    let incoming = machine();
    incoming.session.board.write_register(0, arm::CORE[0], 0x1234).unwrap();
    incoming.session.board.write_physical(0x3000, &[1, 2, 3, 4]).unwrap();
    let live_before_hit = proof(&incoming);
    assert_ne!(live_before_hit, restored_proof);
    assert_eq!(snapshot_digest(&directory.join(SNAPSHOT)).unwrap(), saved_digest);
    let hit = initialized_reset(incoming, &identity, &root.0, None, ResetCachePolicy::ReuseOrCreate).unwrap();
    assert_eq!(hit.cache.lookup(), CacheLookup::Hit);
    assert_eq!(hit.cache.restore(), ResetRestore::Restored);
    assert_eq!(serde_json::to_value(hit.cache).unwrap(), json!({"lookup":"hit", "restore":"restored"}));
    assert_eq!(proof(&hit.machine), restored_proof);
    assert_ne!(proof(&hit.machine), live_before_hit);
    assert_eq!(std::fs::read(directory.join(SNAPSHOT)).unwrap(), saved_bytes);

    let repeat = initialized_reset(hit.machine, &identity, &root.0, None, ResetCachePolicy::ReuseOrCreate).unwrap();
    assert_eq!(proof(&repeat.machine), restored_proof);
}

#[test]
fn changed_identity_misses_instead_of_rebinding_saved_cache() {
    let root = CacheRoot::new();
    let original_identity = identity();
    let original = initialized_reset(machine(), &original_identity, &root.0, None, ResetCachePolicy::ReuseOrCreate).unwrap();
    let original_proof = proof(&original.machine);
    let changed_identity = BootIdentity::new(b"changed-run", b"synthetic-dat", b"synthetic-rom", b"synthetic-nand", None, b"zero-instruction-reset-test");
    let changed = machine();
    changed.session.board.write_register(1, arm::CORE[0], 7).unwrap();
    let changed_proof = proof(&changed);
    let reset = initialized_reset(changed, &changed_identity, &root.0, None, ResetCachePolicy::ReuseOrCreate).unwrap();
    assert_eq!(reset.cache.lookup(), CacheLookup::Miss);
    assert_eq!(reset.cache.restore(), ResetRestore::NotRestored);
    assert_eq!(proof(&reset.machine), changed_proof);
    assert_ne!(changed_proof, original_proof);
    let hit = initialized_reset(machine(), &original_identity, &root.0, None, ResetCachePolicy::ReuseOrCreate).unwrap();
    assert_eq!(proof(&hit.machine), original_proof);
}

#[test]
fn invalid_saved_identity_snapshot_and_budget_refuse_without_replacing_cache() {
    let root = CacheRoot::new();
    let identity = identity();
    initialized_reset(machine(), &identity, &root.0, None, ResetCachePolicy::ReuseOrCreate).unwrap();
    let directory = root.0.join(format!("reset-{}", identity.key()));
    let snapshot_path = directory.join(SNAPSHOT);
    let record_path = directory.join("initialized-reset.json");
    let saved = std::fs::read(&snapshot_path).unwrap();
    let record = std::fs::read(&record_path).unwrap();
    let tiny = CheckpointBudget::new(1).unwrap();
    assert!(initialized_reset(machine(), &identity, &root.0, Some(tiny), ResetCachePolicy::RequireHitRestored).is_err());
    assert_eq!(std::fs::read(&snapshot_path).unwrap(), saved);
    std::fs::write(&record_path, b"{}").unwrap();
    assert!(initialized_reset(machine(), &identity, &root.0, None, ResetCachePolicy::ReuseOrCreate).is_err());
    assert_eq!(std::fs::read(&snapshot_path).unwrap(), saved);
    std::fs::write(&record_path, &record).unwrap();
    std::fs::write(&snapshot_path, b"not a snapshot").unwrap();
    assert!(initialized_reset(machine(), &identity, &root.0, None, ResetCachePolicy::ReuseOrCreate).is_err());
    assert_eq!(std::fs::read(&snapshot_path).unwrap(), b"not a snapshot");
    assert_eq!(std::fs::read(&record_path).unwrap(), record);
}

#[test]
fn rejected_miss_does_not_publish_a_reset_cache() {
    let root = CacheRoot::new();
    let identity = identity();
    let result = initialized_reset(machine(), &identity, &root.0, Some(CheckpointBudget::new(1).unwrap()), ResetCachePolicy::ReuseOrCreate);
    assert!(result.is_err());
    assert!(!root.0.join(format!("reset-{}", identity.key())).exists());
}

#[test]
fn strict_arm_policy_refuses_miss_without_creating_a_cache() {
    let root = CacheRoot::new();
    let identity = identity();
    let result = initialized_reset(machine(), &identity, &root.0, None, ResetCachePolicy::RequireHitRestored);
    assert!(result.is_err());
    assert_eq!(std::fs::read_dir(&root.0).unwrap().count(), 0);
}

#[test]
fn both_arms_restore_the_same_prepared_zero_cache_without_rewriting_it() {
    let root = CacheRoot::new();
    let identity = identity();
    let prepared = initialized_reset(machine(), &identity, &root.0, None, ResetCachePolicy::ReuseOrCreate).unwrap();
    let expected = proof(&prepared.machine);
    let directory = root.0.join(format!("reset-{}", identity.key()));
    let raw_before = std::fs::read(directory.join(SNAPSHOT)).unwrap();
    for _arm in 0..2 {
        let arm = initialized_reset(machine(), &identity, &root.0, None, ResetCachePolicy::RequireHitRestored).unwrap();
        assert_eq!(arm.cache.lookup(), CacheLookup::Hit);
        assert_eq!(arm.cache.restore(), ResetRestore::Restored);
        assert_eq!(proof(&arm.machine), expected);
        assert_eq!(std::fs::read(directory.join(SNAPSHOT)).unwrap(), raw_before);
    }
    let wrong = BootIdentity::new(b"wrong-run", b"synthetic-dat", b"synthetic-rom", b"synthetic-nand", None, b"zero-instruction-reset-test");
    assert!(initialized_reset(machine(), &wrong, &root.0, None, ResetCachePolicy::RequireHitRestored).is_err());
    assert!(!root.0.join(format!("reset-{}", wrong.key())).exists());
    assert_eq!(std::fs::read(directory.join(SNAPSHOT)).unwrap(), raw_before);
}

#[test]
fn offline_native_consumer_authenticates_complete_bound_state_and_refuses_tampering() {
    use sha2::{Digest, Sha256};
    use std::{ffi::OsString, io::Read};
    let machine = machine();
    let original = proof(&machine);
    let mut file = std::fs::File::open(std::env::current_exe().unwrap()).unwrap();
    let mut buffer = [0; 65536];
    let mut hash = Sha256::new();
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 { break; }
        hash.update(&buffer[..count]);
    }
    let executable = format!("{:x}", hash.finalize());
    let args = [
        "--validate-machine-fingerprint".to_owned(),
        "--expected-checkpoint-model".to_owned(), env!("XM1_CHECKPOINT_MODEL_ID").to_owned(),
        "--expected-definition".to_owned(), format!("{:x}", Sha256::digest(b"null")),
        "--expected-executable".to_owned(), executable.clone(),
    ].map(OsString::from);
    let validate = |value: &Value| {
        let input = serde_json::to_vec(value).unwrap();
        fujimod_xm1::observation_contract::execute(&args, input.as_slice()).unwrap()
    };
    let accepted = serde_json::to_value(validate(&original).unwrap()).unwrap();
    assert_eq!(accepted, json!({"schema":1,"status":"complete",
        "checkpoint_model_id":env!("XM1_CHECKPOINT_MODEL_ID"),
        "model_definition_sha256":format!("{:x}", Sha256::digest(b"null")),
        "executable_sha256":executable,"machine_sha256":original["machine_sha256"],
        "counts":[0,0],"now":0}));
    for altered in [
        { let mut value = original.clone(); value.as_object_mut().unwrap().remove("exact_state"); value },
        { let mut value = original.clone(); value["unknown"] = json!(true); value },
        { let mut value = original.clone(); value["status"] = json!("blocked"); value },
        { let mut value = original.clone(); value["counts"] = json!([1,0]); value },
        { let mut value = original.clone(); value["exact_state"]["cpu"][0]["bytes_hex"] = json!("00"); value },
        { let mut value = original.clone(); value["exact_state"]["executable_sha256"] = json!("00".repeat(32)); value },
        { let mut value = original.clone(); value["components"]["root_sha256"] = json!("00".repeat(32)); value },
    ] {
        assert!(validate(&altered).is_err());
    }
    let mut trailing = serde_json::to_vec(&original).unwrap();
    trailing.push(b'x');
    assert!(fujimod_xm1::observation_contract::execute(&args, trailing.as_slice()).unwrap().is_err());
    assert_eq!(machine.session.board.instruction_counts(), vec![0,0]);
    assert_eq!(machine.session.board.now(), 0);
}
