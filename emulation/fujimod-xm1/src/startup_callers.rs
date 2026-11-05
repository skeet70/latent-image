//! Bounded natural-reset caller observations for X-M1 1.50.
use fujimod_emulator::board::observation::{ViewPolicy, WatchKinds, WatchRange};
use fujimod_xm1::bridge::{read, sha256, write};
use fujimod_xm1::guest::Press;
use fujimod_xm1::machine::{KeyPulse, Machine};
use fujimod_xm1::provenance::{self, Form, Guest};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use fujimod_xm1::trace::Hit;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::{Path, PathBuf}, process::ExitCode};

const USAGE: &str = "fujimod-xm1-startup-callers --run RUN --update DAT --boot-rom ROM --analysis ROOT --output DIR [--primary-image BIN [--primary-leaf-reads]]";
const LIMIT: usize = 4096;
const SOURCES: [(&str, u32, &str); 7] = [
    ("decode/boot-40048.bin", 0, "c2da900dcd3f193b05b5ae968dfcb87723571462eafd84c0fed8467f66f7dff3"),
    ("decode/boot-50048.bin", 0x10000, "2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4"),
    ("decode/loader-40000.bin", 0x40000, "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"),
    ("decode/loader-d0000.bin", 0xd0000, "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec"),
    ("lz/id4-0061c400.bin", 0x61c400, "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"),
    ("lz/id5-00bf1a00.bin", 0xbf1a00, "3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a"),
    ("lz/plain-01100000.bin", 0x1100000, "ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d"),
];

#[derive(Serialize)]
#[serde(transparent)]
struct Digest(String);
#[derive(Serialize)]
#[serde(transparent)]
struct Address(u32);
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ReferenceKind { Word, Formed, Branch }
#[derive(Serialize)]
struct Reference { kind: ReferenceKind, at: Address, target: Address }
#[derive(Serialize)]
struct Census { source: &'static str, sha256: Digest, references: Vec<Reference> }
#[derive(Serialize)]
struct Identity { run: Digest, update: Digest, boot_rom: Digest, observer: Digest }
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PrimaryRole { Retained, OwnedV10 }
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum LeafDataReads { Disabled, Observe }
#[derive(Serialize)]
struct PrimaryImage { role: PrimaryRole, image_sha256: Option<Digest> }
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Access { CpuFetch, CpuWrite, CpuRead, HostRead, HostWrite, ModelBusRead, ModelBusWrite }
#[derive(Deserialize)]
#[serde(transparent)]
struct WatchAddress(u64);
#[derive(Deserialize)]
#[serde(transparent)]
struct WatchLength(u64);
#[derive(Deserialize)]
struct Record { source: Access, address: WatchAddress, physical: WatchAddress, size: WatchLength, core: Option<usize>, pc: Option<WatchAddress> }
#[derive(Serialize)]
struct CodeConfinement { fetches: [usize; 2], word_samples: [usize; 2], invocations: [usize; 2], data_reads: [usize; 2] }

fn confinement(state: &serde_json::Value, hits: &[Hit]) -> Result<CodeConfinement, String> {
    let rows = state.get("records").and_then(serde_json::Value::as_array).ok_or("watch records are missing")?;
    let mut observed = false;
    let mut counts = CodeConfinement { fetches: [0; 2], word_samples: [0; 2], invocations: [0; 2], data_reads: [0; 2] };
    for raw in rows {
        let row = Record::deserialize(raw).map_err(|error| error.to_string())?;
        if row.physical.0 >= 0x409ac || row.physical.0.checked_add(row.size.0).ok_or("watch span overflow")? <= 0x40430 { continue; }
        if matches!(row.source, Access::CpuFetch) {
            if row.address.0 != row.physical.0 { return Err("primary fetch uses an unsampled executable alias".into()); }
            let core = row.core.filter(|core| *core < 2).ok_or("leaf fetch has invalid core")?;
            counts.fetches[core] += 1;
            observed = true;
        } else if matches!(row.source, Access::CpuRead) {
            let core = row.core.filter(|core| *core < 2).ok_or("leaf data read has invalid core")?;
            if !row.pc.is_some_and(|pc| u32::try_from(pc.0).is_ok()) { return Err("leaf data read lacks a valid reader PC".into()); }
            counts.data_reads[core] += 1;
        } else if observed && matches!(row.source, Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite) {
            return Err("primary image write after its first observed fetch".into());
        }
    }
    for hit in hits.iter().filter(|hit| hit.code_word.is_some()) {
        if hit.core >= 2 { return Err("code word has invalid core".into()); }
        counts.word_samples[hit.core] += 1;
        if hit.address == "0x40430" { counts.invocations[hit.core] += 1; }
    }
    if !observed || counts.word_samples[0] + counts.word_samples[1] == 0 { return Err("primary code observation is absent".into()); }
    Ok(counts)
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome { ObservedMenu, Deadline, Refused }
#[derive(Deserialize, Serialize)]
struct WatchSummary { enabled: bool, violation: Option<String> }
#[derive(Serialize)]
struct Receipt<'a> {
    schema: u32, scope: &'static str, outcome: Outcome, error: Option<&'a str>,
    machine_failure: Option<&'a str>, instructions: u64, identity: &'a Identity,
    checkpoint_sha256: Digest, checkpoint_reused: bool, sites: &'a BTreeMap<u32, String>,
    primary: PrimaryImage, primary_code: Option<Result<CodeConfinement, String>>,
    primary_leaf_reads: LeafDataReads,
    hits: &'a [Hit], watch: Result<WatchSummary, String>, watch_end_error: Option<String>,
    final_menu: Result<Option<[u32; 3]>, String>,
}

fn wanted(value: Guest) -> bool {
    (0x69cdf8..0x69cfa4).contains(&value.0) || (0xd0430..0xd0b00).contains(&value.0)
        || matches!(value.0, 0x69cc6c | 0xd0020 | 0xd0c8c | 0xd0cac | 0x40c8c | 0x40cac
            | 0xdf0cc | 0x400df0cc | 0xdf0d0 | 0x400df0d0 | 0xdf0d4 | 0x400df0d4
            | 0x4f10c | 0x4004f10c | 0x4f110 | 0x4004f110 | 0x89f794 | 0x4089f794)
}

fn census(root: &Path) -> Result<Vec<Census>, String> {
    SOURCES.into_iter().map(|(source, base, expected)| {
        let bytes = read(&root.join(source))?;
        let digest = sha256(&bytes);
        if digest != expected { return Err(format!("noncanonical source {source}")); }
        let mut references: Vec<_> = provenance::references(Guest(base), &bytes, &wanted).into_iter().map(|reference| Reference {
            kind: match reference.form { Form::Word => ReferenceKind::Word, Form::Code => ReferenceKind::Formed },
            at: Address(reference.at.0), target: Address(reference.value.0),
        }).collect();
        for (offset, chunk) in bytes.chunks_exact(4).enumerate() {
            let word = u32::from_le_bytes(chunk.try_into().map_err(|_| "unaligned census word")?);
            if word >> 28 != 15 && word & 0x0e00_0000 == 0x0a00_0000 {
                let at = base + 4 * offset as u32;
                let displacement = ((word << 8) as i32 >> 6) as u32;
                let target = at.wrapping_add(8).wrapping_add(displacement);
                if wanted(Guest(target)) { references.push(Reference { kind: ReferenceKind::Branch, at: Address(at), target: Address(target) }); }
            }
        }
        Ok(Census { source, sha256: Digest(digest), references })
    }).collect()
}

fn sites() -> BTreeMap<u32, String> {
    [
        (0x69cdf8, "dispatch"), (0x69cf84, "callback-setter"),
        (0x725dd8, "task-wrapper"), (0x69cf7c, "dispatch-result"),
        (0xd0020, "loader-b-entry"), (0xd09ac, "loader-b-startup"),
        (0xd09dc, "loader-b-early-call"), (0xd0aac, "loader-b-late-call"),
        (0xd0430, "loader-b-initializer"),
        (0xd0c8c, "loader-b-flag-setter"), (0xd0cac, "loader-b-diagnostic"),
        (0xd0d2c, "loader-b-restore-frame"), (0xd0d38, "loader-b-restore-pc"),
        (0x40c8c, "loader-a-flag-setter"), (0x40cac, "loader-a-diagnostic"),
        (0x40d2c, "loader-a-restore-frame"), (0x40d38, "loader-a-restore-pc"),
        (0xf8cd9c, "arena-supplied-base"), (0xf8ce24, "arena-base-add-load"),
        (0xf8ce28, "arena-base-add"), (0xf8ce2c, "arena-derived-store-pre"), (0xf8ce30, "arena-derived-store"),
        (0xf9f3c8, "arena-copy-root"), (0xf9f3cc, "arena-copy-base"),
        (0xf9fab8, "arena-copy0-offset"), (0xf9fabc, "arena-copy0-add"), (0xf9fac0, "arena-copy0-size"), (0xf9facc, "arena-copy0-call"),
        (0xf9fad4, "arena-copy1-offset"), (0xf9fad8, "arena-copy1-add"), (0xf9fadc, "arena-copy1-size"), (0xf9fae8, "arena-copy1-call"),
        (0xfa4b68, "arena-write-root"), (0xfa4b6c, "arena-write-base"),
        (0xfa5080, "arena-write0-offset"), (0xfa5084, "arena-write0-add"), (0xfa5090, "arena-write0-store"),
        (0xfa5094, "arena-write1-offset"), (0xfa5098, "arena-write1-add"), (0xfa50a4, "arena-write1-store"),
        (0xfa367c, "arena-global-clear"), (0xfa36a0, "arena-root-publication"),
        (0xfa3edc, "arena-base-source"), (0xfa3ee0, "arena-base-load"), (0xfa3ee4, "arena-base-offset"), (0xfa3ee8, "arena-base-add"),
        (0xfa3eec, "arena-base-publication"), (0xfa3efc, "arena-base-normalize"), (0xfa3f00, "arena-normalized-publication"),
        (0xf9a758, "arena-descriptor-root"), (0xf9a75c, "arena-descriptor-base"), (0xf9a760, "arena-descriptor-publication"),
    ].into_iter().map(|(address, label)| (address, label.into())).collect()
}

fn route(machine: &mut Machine) -> Result<Outcome, String> {
    machine.run(80_000_000);
    machine.require_running()?;
    let initial = machine.menu()?;
    if !initial.is_some_and(|menu| (menu.menu_id, menu.setting_id, menu.cursor) == (69, 52, 12)) {
        return Ok(Outcome::Deadline);
    }
    let pulse = KeyPulse::instructions(8_000_000, 8_000_000);
    for key in std::iter::repeat_n("up", 12).chain(["ok", "ok"]) {
        machine.press_once(Press::parse(key)?, pulse)?;
    }
    machine.run(40_000_000);
    machine.require_running()?;
    machine.press_once(Press::parse("menu")?, pulse)?;
    machine.run(90_000_000);
    machine.require_running()?;
    Ok(if machine.menu()?.is_some_and(|menu| (menu.menu_id, menu.setting_id, menu.cursor) == (1, 5, 2)) {
        Outcome::ObservedMenu
    } else { Outcome::Deadline })
}

fn drive() -> Result<(), String> {
    let mut arguments = BTreeMap::new();
    let mut leaf_data_reads = LeafDataReads::Disabled;
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let flag = flag.into_string().map_err(|_| USAGE)?;
        if flag == "--primary-leaf-reads" {
            if matches!(leaf_data_reads, LeafDataReads::Observe) { return Err(USAGE.into()); }
            leaf_data_reads = LeafDataReads::Observe;
            continue;
        }
        if !["--run", "--update", "--boot-rom", "--analysis", "--output", "--primary-image"].contains(&flag.as_str())
            || arguments.insert(flag, PathBuf::from(args.next().ok_or(USAGE)?)).is_some() { return Err(USAGE.into()); }
    }
    let primary_path = arguments.remove("--primary-image");
    let mut take = |flag| arguments.remove(flag).ok_or_else(|| USAGE.to_owned());
    let run_path = take("--run")?;
    let update_path = take("--update")?;
    let rom_path = take("--boot-rom")?;
    let analysis = take("--analysis")?;
    let output = take("--output")?;
    std::fs::create_dir(&output).map_err(|error| error.to_string())?;
    write(&output.join("source-census.json"), &serde_json::to_vec_pretty(&census(&analysis)?).map_err(|error| error.to_string())?)?;
    let run_bytes = read(&run_path)?;
    let update = read(&update_path)?;
    let rom = read(&rom_path)?;
    let identity = Identity { run: Digest(sha256(&run_bytes)), update: Digest(sha256(&update)),
        boot_rom: Digest(sha256(&rom)), observer: Digest(sha256(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?)) };
    if identity.run.0 != "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
        || !["43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490",
            "12e9e62c27eca01106ab77170c7bcf5275709cd9db8b67d20072cfa270c226e5"].contains(&identity.update.0.as_str())
        || identity.boot_rom.0 != "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be" { return Err("observer requires canonical X-M1 inputs".into()); }
    let (primary, primary_bytes) = match (identity.update.0.as_str(), primary_path) {
        ("43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490", None) =>
            (PrimaryImage { role: PrimaryRole::Retained, image_sha256: None }, None),
        ("12e9e62c27eca01106ab77170c7bcf5275709cd9db8b67d20072cfa270c226e5", Some(path)) => {
            let bytes = read(&path)?;
            let digest = sha256(&bytes);
            let encoded = update.get(525964..525964 + bytes.len()).ok_or("owned image leaves staged DAT")?;
            if bytes.len() != 1404 || digest != "a93e2898f24802220520c4b472820e7cd88a2a675e09dd71c2b5c48a8b6c3503"
                || !bytes.iter().zip(encoded).all(|(plain, stored)| *plain == !*stored) {
                return Err("owned primary image or exact staged placement differs".into());
            }
            (PrimaryImage { role: PrimaryRole::OwnedV10, image_sha256: Some(Digest(digest)) }, Some(bytes))
        },
        _ => return Err("primary image argument does not match the selected DAT role".into()),
    };
    if matches!(leaf_data_reads, LeafDataReads::Observe) && !matches!(primary.role, PrimaryRole::OwnedV10) {
        return Err("--primary-leaf-reads requires the owned primary image".into());
    }
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(update), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    let checkpoint = output.join("reset.fujimod");
    machine.disarm()?;
    let snapshot = machine.session.checkpoint(None)?.export()?;
    write(&checkpoint, &snapshot)?;
    let restored = machine.session.import_snapshot(&read(&checkpoint)?)?;
    machine.session.restore(&restored)?;
    machine.rearm()?;
    if machine.instructions() != 0 { return Err("checkpoint is not preexecution reset".into()); }
    let mut sites = sites();
    if let Some(bytes) = primary_bytes {
        sites.extend((0x40430..0x409ac).step_by(4).map(|address| (address, "primary-code-word".into())));
        sites.extend([
            (0x69ce0c,"phase0-mode"),(0x69ce18,"phase0-clear"),(0x69ce34,"phase0-install"),(0x69ce38,"phase0-installed"),
            (0x725a04,"installation-entry"),(0x725a34,"installation-roots"),(0x725a40,"installation-cache-return"),
            (0x725a48,"installation-class"),(0x725a54,"installation-class-refusal"),(0x725a58,"installation-local"),
            (0x725b40,"installation-return"),
            (0x409f8,"loader-a-phase0"),(0x40a00,"loader-a-phase0-return"),
        ].into_iter().map(|(address,label)| (address,label.into())));
        machine.traces.bind_code_words(std::num::NonZeroUsize::new(16_384).ok_or("invalid trace limit")?, Guest(0x40430), bytes, maps.clone())?;
    } else {
        machine.traces.bound_full_registers(std::num::NonZeroUsize::new(LIMIT).ok_or("invalid trace limit")?);
    }
    machine.traces.arm(&machine.session, sites.clone())?;
    let mut classes = [(0xdf0cc,0xdf0d8), (0x4f10c,0x4f118), (0x89f794,0x89f798),
        (0x16d7370,0x16d7374), (0x16d8284,0x16d8288), (0x16d7d20,0x16d7d24)].map(|(start,end)| WatchRange {
        start, end, kinds: WatchKinds::WRITES,
    }).to_vec();
    let leaf_kinds = WatchKinds::CPU_FETCH.union(WatchKinds::WRITES);
    let leaf_kinds = match leaf_data_reads {
        LeafDataReads::Disabled => leaf_kinds,
        LeafDataReads::Observe => leaf_kinds.union(WatchKinds::CPU_READ),
    };
    classes.push(WatchRange { start: 0x40430, end: 0x409ac, kinds: leaf_kinds });
    if matches!(primary.role, PrimaryRole::OwnedV10) {
        classes.extend([(0x61c008,0x61c00c),(0x40010,0x40014),(0xd0010,0xd0014),(0x7caf3c,0x7caf40)]
            .map(|(start,end)| WatchRange { start, end, kinds: WatchKinds::CPU_READ }));
        classes.push(WatchRange { start: 0x96bd20, end: 0x96bd28, kinds: WatchKinds::WRITES });
        classes.extend([(0xa0f86c,0xa0fc44),(0xa0fc44,0xa101e4),(0x9c3500,0x9c3504),
            (0x9c3604,0x9c3704),(0x9c3804,0x9c4444)].map(|(start,end)| WatchRange { start, end, kinds: WatchKinds::CPU_WRITE }));
    }
    let watch_limit = if matches!(primary.role, PrimaryRole::OwnedV10) { 65_536 } else { 16_384 };
    let mut watch = machine.session.board.memory_watch_begin_classified(classes, watch_limit, maps, ViewPolicy::Observe)?;
    let outcome = route(&mut machine);
    let state = watch.records.borrow().state();
    let summary = if state.get("enabled").is_none() || state.get("violation").is_none() {
        Err("watch state fields missing".into())
    } else { WatchSummary::deserialize(&state).map_err(|error| error.to_string()) };
    write(&output.join("writes.json"), &serde_json::to_vec_pretty(&state).map_err(|error| error.to_string())?)?;
    let watch_end_error = machine.session.board.memory_watch_end(&mut watch).err();
    let final_menu = machine.menu().map(|menu| menu.map(|menu| [menu.menu_id,menu.setting_id,menu.cursor]));
    let primary_code = matches!(primary.role, PrimaryRole::OwnedV10).then(|| confinement(&state, &machine.traces.hits));
    let refused = !summary.as_ref().is_ok_and(|watch| watch.enabled && watch.violation.is_none())
        || watch_end_error.is_some() || final_menu.is_err() || primary_code.as_ref().is_some_and(Result::is_err);
    let result = match &outcome { Ok(result) if !refused => *result, _ => Outcome::Refused };
    let receipt = Receipt { schema: 1, scope: "one-canonical-natural-reset-to-menu-route-only", outcome: result,
        error: outcome.as_ref().err().map(String::as_str), machine_failure: machine.failure.as_deref(), instructions: machine.instructions(),
        identity: &identity, checkpoint_sha256: Digest(sha256(&snapshot)), checkpoint_reused: true, sites: &sites,
        primary, primary_code, primary_leaf_reads: leaf_data_reads,
        hits: &machine.traces.hits, watch: summary, watch_end_error, final_menu };
    write(&output.join("startup-callers.json"), &serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?)?;
    match result {
        Outcome::ObservedMenu => Ok(()),
        Outcome::Deadline => Err("natural route did not reach the shooting menu".into()),
        Outcome::Refused => Err("caller observation refused; inspect the preserved receipt".into()),
    }
}

fn main() -> ExitCode {
    match drive() { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("{error}"); ExitCode::FAILURE } }
}

#[cfg(test)]
mod tests {
    use super::confinement;
    use fujimod_xm1::trace::{CodeWord, Hit};
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn primary_code_admits_canonical_fetch_and_refuses_unsampled_alias() {
        let hit = Hit { site: "primary-code-word".into(), address: "0x40430".into(), core: 0,
            instructions: 1, registers: BTreeMap::new(), sp: "0x40063450".into(),
            lr: "0x409e0".into(), cpsr: 0xd3,
            code_word: Some(CodeWord { guest: 0xe320f000, physical: 0xe320f000, expected: 0xe320f000, tcm: [[0; 2]; 2] }), memory: Vec::new(), observation: None };
        let canonical = json!({"records":[{"source":"cpu-fetch","core":0,"address":0x40430,"physical":0x40430,"size":4}]});
        let alias = json!({"records":[{"source":"cpu-fetch","core":0,"address":0x40040430,"physical":0x40430,"size":4}]});
        assert!(confinement(&canonical, std::slice::from_ref(&hit)).is_ok());
        assert!(confinement(&alias, std::slice::from_ref(&hit)).is_err());
        let mut reads = json!({"records":[
            {"source":"cpu-fetch","core":0,"address":0x40430,"physical":0x40430,"size":4},
            {"source":"cpu-read","core":0,"pc":0x40488,"address":0x40564,"physical":0x40564,"size":4},
            {"source":"cpu-read","core":1,"pc":0xd090c,"address":0x40040564,"physical":0x40564,"size":4}
        ]});
        assert_eq!(confinement(&reads, std::slice::from_ref(&hit)).unwrap().data_reads, [1, 1]);
        reads["records"][1]["pc"] = serde_json::Value::Null;
        assert!(confinement(&reads, std::slice::from_ref(&hit)).is_err());
        reads["records"][1]["pc"] = json!(0x40488);
        reads["records"][1]["core"] = serde_json::Value::Null;
        assert!(confinement(&reads, std::slice::from_ref(&hit)).is_err());
    }
}
