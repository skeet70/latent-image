//! `//emulation:updater-f7` on FujiMod's EXR II runtime. Phase A continues the
//! `ram-rescue-engine` raw handoff with the restore card and the installed
//! committed NAND, selects BODY with OK, OK, UP, OK, OK and runs until the
//! whole-area commit has run and the NAND is idle, then exports the committed
//! NAND in the retained format. Phase B cold-boots a new session from that
//! exported NAND alone to the shooting screen. Sites, schedule and receipt
//! fields follow `emulation/UpdaterF7.hs`.
use fujimod_xm1::bridge::{load_card, read, sha256, write};
use fujimod_xm1::guest::RouteKey;
use fujimod_xm1::handoff::Handoff;
use fujimod_xm1::machine::{KeyPulse, Machine};
use fujimod_xm1::nand_image::CommittedImage;
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use fujimod_xm1::sd_host::Backing;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

const USAGE: &str = "Usage: fujimod-xm1-updater --run RUN.json --boot-rom ROM --firmware STOCK.DAT \
--recovery-ram RAM --sd-image CARD --installed-nand NAND --output DIR\n";

const COMPLETION: u32 = 0x00db91e0;
const HEADER_PARSER: u32 = 0x00db9cd4;
const TEMPLATE_RELOAD: u32 = 0x0069fe20;
const WHOLE_AREA_COMMIT: u32 = 0x00db932c;
/// Kind-0 flag block; the flag word is at +0xc.
const FLAG_BLOCK: u64 = 0x014e356c;
const CACHE_POINTER: u64 = 0x0096ca50;
const SYSTEM_MODE: u64 = 0x009570dc;
const CONNECT_HOOK: u64 = 0x00ecf900;
const STORED_OFFSETS: [usize; 3] = [0x80, 0x81, 0xf7];
/// The stored-f7 fixture holds the area's first logical block at physical block 0x381.
const AREA_FIRST_BLOCK: u32 = 0x381;
const PAGES_PER_BLOCK: u32 = 64;

const CHUNK: u64 = 15_000_000;
const UPDATER_SCREEN_AT: u64 = 135_000_000;
const COMPLETION_BOUND: u64 = 3_500_000_000;
const FIRST_START_AT: u64 = 100_000_000;
const SHOOTING_SETTLE: u64 = 45_000_000;
const PULSE: KeyPulse = KeyPulse { hold: 8_000_000, release: 8_000_000 };
/// OK, OK, UP, OK, OK on the updater screen selects BODY.
const UPDATER_PRESSES: &[RouteKey] = &[RouteKey::Ok, RouteKey::Ok, RouteKey::Up, RouteKey::Ok, RouteKey::Ok];
const COMPLETION_LABEL: &str = "completion";
const COMMIT_LABEL: &str = "whole-area-commit";
const RELOAD_LABEL: &str = "template-reload";

struct Arguments {
    run: PathBuf,
    boot_rom: PathBuf,
    firmware: PathBuf,
    recovery_ram: PathBuf,
    sd_image: PathBuf,
    installed_nand: PathBuf,
    output: PathBuf,
}

fn arguments() -> Result<Arguments, String> {
    let mut values: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let flag = flag.to_str().ok_or(USAGE)?.to_owned();
        let value = args.next().map(PathBuf::from).ok_or(USAGE)?;
        if values.insert(flag, value).is_some() {
            return Err(USAGE.into());
        }
    }
    let mut take = |name: &str| values.remove(name).ok_or_else(|| USAGE.to_owned());
    let arguments = Arguments {
        run: take("--run")?,
        boot_rom: take("--boot-rom")?,
        firmware: take("--firmware")?,
        recovery_ram: take("--recovery-ram")?,
        sd_image: take("--sd-image")?,
        installed_nand: take("--installed-nand")?,
        output: take("--output")?,
    };
    if !values.is_empty() {
        return Err(USAGE.into());
    }
    Ok(arguments)
}

struct Inputs {
    document_bytes: Vec<u8>,
    update: Vec<u8>,
    boot_rom: Vec<u8>,
}

impl Inputs {
    /// The flash base the installed NAND image names: the update's complemented flash bytes.
    fn flash_base(&self, document: &RunDocument) -> Result<Vec<u8>, String> {
        let flash = self.update.get(document.update_flash_offset..).ok_or("update flash offset exceeds input")?;
        Ok(flash.iter().map(|byte| !byte).collect())
    }

    fn machine(&self, nand: &CommittedImage) -> Result<Machine, String> {
        let document = RunDocument::parse(&self.document_bytes)?;
        let mut session = run::open(&document, &ValidatedUpdate(self.update.clone()), &self.boot_rom)?;
        session.board.set_execution_policy(document.execution_policy.into())?;
        nand.apply(&mut session, &document.nand, &self.flash_base(&document)?)?;
        Machine::new(document, session)
    }
}

fn run_until(machine: &mut Machine, target: u64) -> Result<(), String> {
    while machine.instructions() < target {
        machine.run(target - machine.instructions());
        machine.require_running()?;
    }
    Ok(())
}

fn hex_word(value: u32) -> String {
    format!("{value:#x}")
}

/// The adjustment cache base and its stored offsets.
fn cache(machine: &Machine) -> Result<Value, String> {
    let base = machine.word(CACHE_POINTER)?;
    if base == 0 {
        return Err("adjustment cache pointer is not set".into());
    }
    let mut bytes = serde_json::Map::new();
    for offset in STORED_OFFSETS {
        bytes.insert(format!("{offset:#x}"), machine.read(u64::from(base) + offset as u64, 1)?[0].into());
    }
    Ok(json!({"cache_base": hex_word(base), "bytes": bytes}))
}

fn cache_byte(cache: &Value, offset: usize) -> Option<u64> {
    cache["bytes"][format!("{offset:#x}")].as_u64()
}

/// Page 0 of the committed block carrying the area's first logical tag.
fn area_first_page(image: &CommittedImage, tag: &[u8]) -> Result<Value, String> {
    let [block] = image.blocks_tagged(tag)[..] else {
        return Err("expected one committed block with the area's first tag".into());
    };
    let (data, _) = image.page(block * PAGES_PER_BLOCK).ok_or("area block has no committed page 0")?;
    let mut bytes = serde_json::Map::new();
    for offset in STORED_OFFSETS {
        bytes.insert(format!("{offset:#x}"), data[offset].into());
    }
    Ok(json!({"physical_block": format!("{block:#x}"), "bytes": bytes}))
}

fn capture(machine: &Machine, directory: &Path, label: &str) -> Result<Value, String> {
    match machine.frame()? {
        Some(png) => {
            std::fs::create_dir_all(directory).map_err(|e| format!("{}: {e}", directory.display()))?;
            write(&directory.join(format!("{label}.png")), &png)?;
            Ok(json!(sha256(&png)))
        }
        None => Ok(Value::Null),
    }
}

/// Runs chunks until the whole-area commit has run, the controller is idle and
/// the medium is unchanged over one chunk. Returns the completion flag word.
fn run_to_commit_idle(machine: &mut Machine) -> Result<u32, String> {
    let medium = machine.document.nand.0.clone();
    let mut flag = None;
    let mut previous = None;
    loop {
        let end = machine.instructions() + CHUNK;
        while machine.instructions() < end {
            let seen = machine.traces.hits.len();
            machine.run(end - machine.instructions());
            machine.require_running()?;
            if flag.is_none() && machine.traces.hits[seen..].iter().any(|hit| hit.site == COMPLETION_LABEL) {
                flag = Some(machine.word(FLAG_BLOCK + 0xc)?);
            }
        }
        let committed = machine.traces.hits.iter().any(|hit| hit.site == COMMIT_LABEL);
        let busy = machine.session.nand_state(&medium)?["busy"].as_bool().ok_or("NAND state has no busy flag")?;
        let medium_state = machine.session.nand_medium(&medium)?.fingerprint();
        if committed && !busy && previous.as_ref() == Some(&medium_state) {
            return flag.ok_or_else(|| "the update completion 0x00db91e0 did not run".into());
        }
        if machine.instructions() >= COMPLETION_BOUND {
            return Err("the updater did not reach an idle committed NAND within the bound".into());
        }
        previous = Some(medium_state);
    }
}

fn drive() -> Result<bool, String> {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let arguments = arguments()?;
    std::fs::create_dir(&arguments.output).map_err(|e| format!("{}: {e}", arguments.output.display()))?;
    let inputs = Inputs {
        document_bytes: read(&arguments.run)?,
        update: read(&arguments.firmware)?,
        boot_rom: read(&arguments.boot_rom)?,
    };
    let installed_bytes = read(&arguments.installed_nand)?;
    let installed = CommittedImage::parse(&installed_bytes)?;
    let tag: Vec<u8> = installed
        .page(AREA_FIRST_BLOCK * PAGES_PER_BLOCK)
        .map(|(_, spare)| spare[..4].to_vec())
        .ok_or("installed NAND has no committed spare page at block 0x381")?;
    let stored_before = area_first_page(&installed, &tag)?;

    let started = Instant::now();
    let mut updater = inputs.machine(&installed)?;
    updater.insert_card(load_card(&inputs.document_bytes, &arguments.sd_image, Backing::Complete)?)?;
    let handoff = Handoff::load(&arguments.recovery_ram)?;
    handoff.apply(&mut updater)?;
    let sites: BTreeMap<u32, String> = [
        (COMPLETION, COMPLETION_LABEL),
        (HEADER_PARSER, "header-parser"),
        (TEMPLATE_RELOAD, RELOAD_LABEL),
        (WHOLE_AREA_COMMIT, COMMIT_LABEL),
    ]
    .into_iter()
    .map(|(address, label)| (address, label.to_owned()))
    .collect();
    updater.traces.stop_labels = [COMPLETION_LABEL.to_owned()].into();
    updater.traces.arm(&updater.session, sites)?;
    let frames = arguments.output.join("updater-frames");
    run_until(&mut updater, UPDATER_SCREEN_AT)?;
    let cache_at_screen = cache(&updater)?;
    let mut updater_frames = vec![json!({"step": "updater screen", "png_sha256": capture(&updater, &frames, "screen")?})];
    for (index, key) in UPDATER_PRESSES.iter().copied().enumerate() {
        updater.key(key, PULSE)?;
        updater_frames.push(json!({"step": format!("after {}", serde_json::to_value(key).map_err(|e| e.to_string())?.as_str().unwrap_or("key")),
            "png_sha256": capture(&updater, &frames, &format!("key-{index}"))?}));
    }
    let flag = run_to_commit_idle(&mut updater)?;
    let cache_after_commit = cache(&updater)?;
    let medium = updater.session.nand_medium(&updater.document.nand.0)?;
    let exported = installed.export(&medium)?;
    let exported_path = arguments.output.join("updated.nand");
    write(&exported_path, &exported)?;
    let hits = updater.traces.hits.clone();
    let updater_instructions = updater.instructions();
    let updater_seconds = started.elapsed().as_secs_f64();
    drop(updater);

    let after = CommittedImage::parse(&exported)?;
    let stored_after = area_first_page(&after, &tag)?;
    let started = Instant::now();
    let mut reboot = inputs.machine(&after)?;
    let frames = arguments.output.join("reboot-frames");
    run_until(&mut reboot, FIRST_START_AT)?;
    let first_start_frame = capture(&reboot, &frames, "first-start")?;
    for key in std::iter::repeat_n(RouteKey::Up, 12).chain([RouteKey::Ok, RouteKey::Ok]) {
        reboot.key(key, PULSE)?;
    }
    let settled = reboot.instructions() + SHOOTING_SETTLE;
    run_until(&mut reboot, settled)?;
    let shooting_frame = capture(&reboot, &frames, "shooting")?;
    let cache_after_reboot = cache(&reboot)?;
    let mode = reboot.word(SYSTEM_MODE)?;
    let hook = reboot.word(CONNECT_HOOK)?;
    let menu = reboot.menu()?.map(|menu| json!({"menu_id": menu.menu_id, "setting_id": menu.setting_id, "cursor": menu.cursor}));

    let completion_ran = [RELOAD_LABEL, COMMIT_LABEL].iter().all(|label| hits.iter().any(|hit| hit.site == *label));
    let stored = |page: &Value| page["bytes"]["0xf7"].as_u64();
    let expectations = [
        ("flag word & 3 == 3 at 0x00db91e0", flag & 3 == 3, json!(hex_word(flag))),
        ("template reload 0x0069fe20 and whole-area commit 0x00db932c ran", completion_ran, Value::Null),
        ("stored 0xf7 before == 1", stored(&stored_before) == Some(1), json!(stored(&stored_before))),
        ("stored 0xf7 after == 0", stored(&stored_after) == Some(0), json!(stored(&stored_after))),
        (
            "cache 0xf7 after reboot == stored 0xf7 after",
            cache_byte(&cache_after_reboot, 0xf7) == stored(&stored_after),
            json!(cache_byte(&cache_after_reboot, 0xf7)),
        ),
    ];
    let passed = expectations.iter().all(|(_, passed, _)| *passed);
    let receipt = json!({
        "schema": 1,
        "scope": "emulated X-M1 1.50 ordinary updater over a committed NAND on FujiMod's EXR II runtime, then a cold boot of the committed result",
        "engine": "fujimod",
        "inputs": {
            "firmware": {"path": arguments.firmware, "sha256": sha256(&inputs.update)},
            "recovery_ram": {"path": arguments.recovery_ram, "sha256": sha256(&read(&arguments.recovery_ram)?), "route": format!("{:?}", handoff.route())},
            "sd_image": {"path": arguments.sd_image, "sha256": sha256(&read(&arguments.sd_image)?)},
            "installed_nand": {"path": arguments.installed_nand, "sha256": sha256(&installed_bytes)},
        },
        "updater": {
            "frames": updater_frames,
            "cache_at_updater_screen": cache_at_screen,
            "completion_flag_word_0x014e3578": hex_word(flag),
            "hits": hits,
            "cache_after_commit": cache_after_commit,
            "instructions": updater_instructions,
            "seconds": updater_seconds,
        },
        "stored_before": stored_before,
        "stored_after": stored_after,
        "area_first_block_tag": tag.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "exported_nand": {"path": exported_path, "sha256": sha256(&exported)},
        "reboot": {
            "first_start_frame": first_start_frame,
            "shooting_frame": shooting_frame,
            "instructions": reboot.instructions(),
            "seconds": started.elapsed().as_secs_f64(),
            "menu": menu,
            "cache": cache_after_reboot,
            "system_mode_word_0x009570dc": mode,
            "connect_hook_word_0x00ecf900": hex_word(hook),
        },
        "expectations": expectations.iter().map(|(name, passed, observed)| json!({"name": name, "passed": passed, "observed": observed})).collect::<Vec<_>>(),
    });
    write(&arguments.output.join("receipt.json"), format!("{receipt:#}\n").as_bytes())?;
    println!("{receipt}");
    Ok(passed)
}

fn main() -> ExitCode {
    match drive() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("fujimod-xm1-updater: an expectation failed; see receipt.json");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("fujimod-xm1-updater: {error}");
            ExitCode::FAILURE
        }
    }
}
