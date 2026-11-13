use super::*;
use fujimod_xm1::guest::{GpioKey, ShutterStage};

const BASE: u32 = fujimod_xm1::fixed_image_layout::IMAGE_BASE;
const END: u32 = 0x00bf1a00;
const BREAK: u32 = 0x00829a08;
const STEP: u64 = 8_000_000;

pub(super) fn boot_fill_once(writes: &mut [(u64, u64)]) -> bool {
    writes.sort_unstable();
    let mut cursor = u64::from(BASE);
    writes.iter().all(|&(low, high)| {
        let contiguous = low == cursor && low < high;
        cursor = high;
        contiguous
    }) && cursor == u64::from(END)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Scenario { Prepare, Capture, Playback, ResetWatch }

struct Observation {
    peak: u32,
    backing_returns: usize,
    saclac_entries: usize,
    guarded_writes: usize,
    boot_writes: Vec<(u64, u64)>,
    loader_entered: bool,
    watch: Option<Box<fujimod_emulator::board::observation::MemoryWatch>>,
    maps: Vec<MemoryMap>,
    hits: usize,
}

impl Observation {
    fn begin(machine: &mut Machine, maps: Vec<MemoryMap>, loader_entered: bool) -> Result<Self, String> {
        machine.traces.bound_full_registers(std::num::NonZeroUsize::new(16_384).ok_or("invalid placement hit bound")?);
        machine.traces.arm(&machine.session, [0x72b158, 0x4072b158, 0x40020, 0x40040020, 0xefca6c, 0x40efca6c].into_iter().map(|address| (address, "placement".into())).collect())?;
        machine.traces.stop_labels = ["placement".into()].into_iter().collect();
        let mut observation = Self { peak: 0xa71a00, backing_returns: 0, saclac_entries: 0, guarded_writes: 0, boot_writes: Vec::new(), loader_entered, watch: None, maps, hits: 0 };
        if loader_entered { observation.sample_break(machine)?; }
        observation.arm_watch(machine)?;
        Ok(observation)
    }

    fn arm_watch(&mut self, machine: &mut Machine) -> Result<(), String> {
        self.watch = Some(machine.session.board.memory_watch_begin_classified(vec![WatchRange {
            start: u64::from(BASE), end: u64::from(END), kinds: WatchKinds::WRITES,
        }], 262_144, self.maps.clone(), ViewPolicy::Observe)?);
        Ok(())
    }

    fn drain(&mut self, machine: &mut Machine) -> Result<(), String> {
        let Some(mut watch) = self.watch.take() else { return Ok(()); };
        let state = watch.records.borrow().state();
        let batch: AccessBatch = serde_json::from_value(serde_json::to_value(state).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
        machine.session.board.memory_watch_end(&mut watch)?;
        require(batch.enabled && batch.violation.is_none(), "placement watch overflow or refusal")?;
        for record in batch.records {
            if matches!(record.source, Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite) {
                let low = record.physical.max(u64::from(BASE));
                let high = record.physical.checked_add(record.size).ok_or("placement write wraps")?.min(u64::from(END));
                require(low < high, "placement watch returned an unrelated write")?;
                if self.loader_entered { self.guarded_writes += 1; }
                else { self.boot_writes.push((low, high)); }
            }
        }
        require(self.guarded_writes == 0, "write entered the reserved partition after loader-A entry")
    }

    fn sample_break(&mut self, machine: &Machine) -> Result<(), String> {
        let value = word(machine, &self.maps, &tcm(machine)?, 0, BREAK)?;
        require((0xa71a00..=END).contains(&value), "heap break leaves its stock envelope")?;
        self.peak = self.peak.max(value);
        Ok(())
    }

    fn advance(&mut self, machine: &mut Machine, count: u64) -> Result<(), String> {
        let target = machine.instructions().checked_add(count).ok_or("placement route counter overflow")?;
        while machine.instructions() < target {
            let outcome = machine.run(target - machine.instructions());
            machine.require_running()?;
            if outcome != RunOutcome::Stopped { break; }
            let hits = &machine.traces.hits[self.hits..];
            require(hits.len() == 1, "placement instruction stop has ambiguous hits")?;
            let address = u32::from_str_radix(hits[0].address.trim_start_matches("0x"), 16).map_err(|error| error.to_string())? & !0x40000000;
            self.drain(machine)?;
            match address {
                0x40020 => self.loader_entered = true,
                0x72b158 => { self.sample_break(machine)?; self.backing_returns += 1; }
                0xefca6c => { self.sample_break(machine)?; self.saclac_entries += 1; }
                _ => return Err("unexpected placement stop".into()),
            }
            self.hits = machine.traces.hits.len();
            self.arm_watch(machine)?;
        }
        require(machine.instructions() == target, "placement route ended before its instruction bound")
    }

    fn key(&mut self, machine: &mut Machine, key: RouteKey) -> Result<(), String> {
        machine.press(key)?;
        self.advance(machine, STEP)?;
        machine.release_key(key)?;
        self.advance(machine, STEP)
    }

    fn gpio(&mut self, machine: &mut Machine, key: GpioKey, pressed: bool) -> Result<(), String> {
        let node = machine.document.gpio.clone();
        machine.session.gpio(&node.0, key.port, key.pin, if pressed { key.pressed_high } else { !key.pressed_high })?;
        self.advance(machine, STEP)
    }

    fn setup(&mut self, machine: &mut Machine) -> Result<(), String> {
        self.advance(machine, 80_000_000)?;
        require(menu(machine)? == Some([69, 52, 12]), "placement initial setup menu differs")?;
        for key in std::iter::repeat_n(RouteKey::Up, 12).chain([RouteKey::Ok, RouteKey::Ok]) { self.key(machine, key)?; }
        self.advance(machine, 40_000_000)
    }

    fn capture(&mut self, machine: &mut Machine) -> Result<(), String> {
        self.key(machine, RouteKey::Ok)?;
        self.advance(machine, 90_000_000)?;
        require(menu(machine)? == Some([1, 5, 2]), "placement shooting menu differs")?;
        for _ in 0..64 {
            if menu(machine)? == Some([1, 5, 20]) { break; }
            self.key(machine, RouteKey::Down)?;
        }
        require(menu(machine)? == Some([1, 5, 20]), "shoot-without-lens row differs")?;
        self.key(machine, RouteKey::Right)?;
        require(menu(machine)? == Some([1, 0xa3, 1]), "shoot-without-lens editor differs")?;
        self.key(machine, RouteKey::Up)?;
        require(menu(machine)? == Some([1, 0xa3, 0]), "shoot-without-lens ON differs")?;
        self.key(machine, RouteKey::Ok)?;
        self.advance(machine, 40_000_000)?;
        self.key(machine, RouteKey::Back)?;
        self.advance(machine, 40_000_000)?;
        let half = machine.document.firmware.shutter(&machine.session, ShutterStage::Half)?;
        let full = machine.document.firmware.shutter(&machine.session, ShutterStage::Full)?;
        self.gpio(machine, half, true)?;
        self.gpio(machine, full, true)?;
        self.gpio(machine, full, false)?;
        self.advance(machine, STEP)?;
        self.gpio(machine, half, false)?;
        self.advance(machine, STEP)
    }
}

pub fn command(arguments: &[std::ffi::OsString]) -> Option<Result<(), String>> {
    if arguments.first().is_none_or(|argument| argument != "--placement") { return None; }
    Some(drive(arguments))
}

fn drive(arguments: &[std::ffi::OsString]) -> Result<(), String> {
    require(arguments.len() == 12, "heap-observe --placement prepare|capture|playback|reset-watch --run RUN --update DAT --boot-rom ROM --checkpoint FILE --output DIR")?;
    let scenario = match arguments[1].to_str() { Some("prepare") => Scenario::Prepare, Some("capture") => Scenario::Capture, Some("playback") => Scenario::Playback, Some("reset-watch") => Scenario::ResetWatch, _ => return Err("unknown placement scenario".into()) };
    let mut options = BTreeMap::new();
    for pair in arguments[2..].chunks_exact(2) {
        let flag = pair[0].to_str().ok_or("non-UTF8 placement flag")?;
        require(["--run", "--update", "--boot-rom", "--checkpoint", "--output"].contains(&flag), "unknown placement flag")?;
        require(options.insert(flag, PathBuf::from(&pair[1])).is_none(), "duplicate placement flag")?;
    }
    let mut take = |flag| options.remove(flag).ok_or_else(|| format!("missing {flag}"));
    let run_bytes = read(&take("--run")?)?;
    let update = read(&take("--update")?)?;
    let rom = read(&take("--boot-rom")?)?;
    let checkpoint = take("--checkpoint")?;
    let output = take("--output")?;
    std::fs::create_dir(&output).map_err(|error| error.to_string())?;
    require(sha256(&run_bytes) == "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef" && sha256(&rom) == "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be", "placement run or ROM differs")?;
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(update.clone()), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    let resume = matches!(scenario, Scenario::Capture | Scenario::Playback);
    let observer_digest = sha256(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?);
    let identity = format!("{}:{}:{}:{}:post-setup-liveview-v1", sha256(&run_bytes), sha256(&update), sha256(&rom), observer_digest);
    if resume {
        require(std::fs::read_to_string(checkpoint.with_extension("identity")).map_err(|error| error.to_string())? == identity, "placement checkpoint identity differs")?;
        machine.disarm()?;
        let snapshot = machine.session.import_snapshot(&read(&checkpoint)?)?;
        machine.session.restore(&snapshot)?;
        machine.rearm()?;
        require(machine.instructions() > 0, "placement scenario needs a post-boot checkpoint")?;
    }
    let mut observer = Observation::begin(&mut machine, maps, resume)?;
    let result = (|| -> Result<(), String> {
        if !resume { observer.setup(&mut machine)?; }
        if scenario != Scenario::ResetWatch {
            let banks = tcm(&machine)?;
            require(word(&machine, &observer.maps, &banks, 0, 0x72b12c)? == 0xe3003000 && word(&machine, &observer.maps, &banks, 0, 0x72b130)? == 0xe34030ba, "placement workload does not have the fixed heap ceiling")?;
        }
        match scenario {
            Scenario::Capture => observer.capture(&mut machine)?,
            Scenario::Playback => { let key = machine.document.firmware.play(&machine.session)?; observer.gpio(&mut machine, key, true)?; observer.gpio(&mut machine, key, false)?; observer.advance(&mut machine, 90_000_000)?; }
            Scenario::Prepare => (),
            Scenario::ResetWatch => { observer.key(&mut machine, RouteKey::Ok)?; observer.advance(&mut machine, 90_000_000)?; require(menu(&machine)? == Some([1, 5, 2]), "reset watch did not reach shooting menu")?; }
        }
        observer.drain(&mut machine)?;
        observer.sample_break(&machine)?;
        require(observer.loader_entered, "loader-A boundary was not observed")?;
        require(observer.peak < BASE - 0x40000, "heap high-water is within 256 KiB of the fixed ceiling")?;
        if scenario == Scenario::Prepare {
            machine.disarm()?;
            write(&checkpoint, &machine.session.checkpoint(None)?.export()?)?;
            write(&checkpoint.with_extension("identity"), identity.as_bytes())?;
        }
        Ok(())
    })();
    let fill_exactly_once = boot_fill_once(&mut observer.boot_writes);
    let complete = result.is_ok() && (resume || fill_exactly_once);
    write(&output.join("placement.json"), &serde_json::to_vec_pretty(&serde_json::json!({
        "status": if complete { "passed" } else { "failed" }, "error": result.err(), "scenario": arguments[1].to_string_lossy(),
        "run_sha256": sha256(&run_bytes), "dat_sha256": sha256(&update), "rom_sha256": sha256(&rom),
        "instructions": machine.session.board.instruction_counts(), "heap_break_high_water": observer.peak,
        "heap_ceiling": BASE, "ceiling_headroom": BASE.saturating_sub(observer.peak), "backing_returns": observer.backing_returns,
        "saclac_boundary_entries": observer.saclac_entries, "observer_sha256": observer_digest,
        "guarded_writes": observer.guarded_writes, "boot_fill_exactly_once": fill_exactly_once, "boot_write_records": observer.boot_writes.len(),
        "checkpoint_restored": resume, "wifi": "open-camera-final-pass"
    })).map_err(|error| error.to_string())?)?;
    require(complete, "placement observation failed; result preserves the boundary")
}
