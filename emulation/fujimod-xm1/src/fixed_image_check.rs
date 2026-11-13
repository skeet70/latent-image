use super::*;
use fujimod_xm1::fixed_image_layout::{IMAGE_BASE, IMAGE_LIMIT};

#[derive(Clone, Copy, Eq, PartialEq)]
enum Scenario { Success, Corrupt }
#[derive(Clone, Copy, Eq, PartialEq)]
enum Advance { Complete, Halted }

struct Symbols {
    load: Address,
    load_limit: Address,
    handoff: Address,
    halt: Address,
    entry: Address,
    data: Address,
    count: Address,
    failures: Address,
}

fn symbol_record(bytes: &[u8], name: &str) -> Result<(Address, Address), String> {
    let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
    let mut matches = text.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        let address = fields.next()?;
        let size = fields.next()?;
        (fields.next_back() == Some(name)).then_some((address, size))
    });
    let (value, size) = matches.next().ok_or_else(|| format!("missing symbol {name}"))?;
    require(matches.next().is_none(), "duplicate fixed check symbol")?;
    let start = u32::from_str_radix(value, 16).map_err(|error| error.to_string())?;
    let bytes = u32::from_str_radix(size, 16).map_err(|error| error.to_string())?;
    Ok((Address(start), Address(start.checked_add(bytes).ok_or("symbol range overflow")?)))
}

fn symbol(bytes: &[u8], name: &str) -> Result<Address, String> {
    symbol_record(bytes, name).map(|(start, _)| start)
}

struct Observer {
    symbols: Symbols,
    maps: Vec<MemoryMap>,
    watch: Option<Box<fujimod_emulator::board::observation::MemoryWatch>>,
    hits: usize,
    load_begin: Option<u64>,
    handoff: Option<u64>,
    first_entry: Option<u64>,
    entries: usize,
    halted: bool,
    fixed_fetches: usize,
    owned_writes: usize,
    boot_writes: Vec<(u64, u64)>,
}

impl Observer {
    fn begin(machine: &mut Machine, symbols: Symbols, maps: Vec<MemoryMap>) -> Result<Self, String> {
        let sites = [symbols.load, symbols.handoff, symbols.halt, symbols.entry].into_iter()
            .flat_map(|address| [address.0, address.0 | 0x40000000]).map(|address| (address, "fixed-image".into())).collect();
        machine.traces.bound_full_registers(std::num::NonZeroUsize::new(4096).ok_or("invalid fixed trace bound")?);
        machine.traces.arm(&machine.session, sites)?;
        machine.traces.stop_labels = ["fixed-image".into()].into_iter().collect();
        let mut observer = Self { symbols, maps, watch: None, hits: 0, load_begin: None, handoff: None, first_entry: None, entries: 0, halted: false, fixed_fetches: 0, owned_writes: 0, boot_writes: Vec::new() };
        observer.arm(machine)?;
        Ok(observer)
    }

    fn arm(&mut self, machine: &mut Machine) -> Result<(), String> {
        self.watch = Some(machine.session.board.memory_watch_begin_classified(vec![WatchRange {
            start: u64::from(IMAGE_BASE), end: 0xbf1a00, kinds: WatchKinds::WRITES.union(WatchKinds::CPU_FETCH),
        }], 262_144, self.maps.clone(), ViewPolicy::Observe)?);
        Ok(())
    }

    fn drain(&mut self, machine: &mut Machine) -> Result<(), String> {
        let Some(mut watch) = self.watch.take() else { return Ok(()); };
        let state = watch.records.borrow().state();
        let batch: AccessBatch = serde_json::from_value(serde_json::to_value(state).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
        machine.session.board.memory_watch_end(&mut watch)?;
        require(batch.enabled && batch.violation.is_none(), "fixed partition watch refused or overflowed")?;
        for record in batch.records {
            match record.source {
                Access::CpuFetch => {
                    require(self.handoff.is_some() && record.physical < u64::from(IMAGE_LIMIT), "fixed image fetch preceded successful load or entered unused guard")?;
                    self.fixed_fetches += 1;
                }
                Access::CpuWrite | Access::HostWrite | Access::ModelBusWrite => {
                    let pc = record.pc.map(|pc| pc & !0x40000000);
                    let end = record.physical.checked_add(record.size).ok_or("fixed write wraps")?;
                    if self.handoff.is_some() {
                        require(matches!(record.source, Access::CpuWrite) && pc.is_some_and(|pc| (u64::from(IMAGE_BASE)..u64::from(IMAGE_LIMIT)).contains(&pc))
                            && end <= u64::from(IMAGE_LIMIT), "foreign write entered reserved partition after handoff")?;
                        self.owned_writes += 1;
                    } else if self.load_begin.is_some() {
                        require(end <= u64::from(IMAGE_LIMIT) && (matches!(record.source, Access::ModelBusWrite)
                            || matches!(record.source, Access::CpuWrite) && pc.is_some_and(|pc| (u64::from(self.symbols.load.0)..u64::from(self.symbols.load_limit.0)).contains(&pc))),
                            "foreign write entered reserved partition during load")?;
                    } else {
                        require(matches!(record.source, Access::ModelBusWrite), "non-DMA writer entered boot fill partition")?;
                        self.boot_writes.push((record.physical.max(u64::from(IMAGE_BASE)), end.min(0xbf1a00)));
                    }
                }
                _ => (),
            }
        }
        Ok(())
    }

    fn value(&self, machine: &Machine, address: Address) -> Result<u32, String> {
        word(machine, &self.maps, &tcm(machine)?, 0, address.0)
    }

    fn advance(&mut self, machine: &mut Machine, count: u64) -> Result<Advance, String> {
        let target = machine.instructions().checked_add(count).ok_or("fixed check counter overflow")?;
        while machine.instructions() < target {
            let outcome = machine.run(target - machine.instructions());
            machine.require_running()?;
            if outcome != RunOutcome::Stopped { break; }
            let hits = &machine.traces.hits[self.hits..];
            require(hits.len() == 1, "fixed image stop was not a unique boundary")?;
            let address = u32::from_str_radix(hits[0].address.trim_start_matches("0x"), 16).map_err(|error| error.to_string())? & !0x40000000;
            require(address == self.symbols.entry.0 || hits[0].core == 0, "first-stage load boundary ran on core 1")?;
            self.drain(machine)?;
            if address == self.symbols.load.0 {
                require(super::heap_placement::boot_fill_once(&mut self.boot_writes), "reserved partition was not filled exactly once before owned load")?;
                require(self.load_begin.replace(machine.instructions()).is_none(), "fixed record loaded twice")?;
            } else if address == self.symbols.handoff.0 {
                require(self.load_begin.is_some() && self.handoff.is_none(), "handoff preceded fixed load or repeated")?;
                require(self.value(machine, self.symbols.data)? == 0x13579bdf && self.value(machine, self.symbols.count)? == 0
                    && self.value(machine, self.symbols.failures)? == 0, "data or BSS is not initialized before handoff")?;
                let branch = self.value(machine, Address(0x69cdf8))?;
                let delta = ((branch & 0xffffff) << 8) as i32 >> 6;
                require(branch >> 24 == 0xea && (0x69cdf8u32 + 8).wrapping_add(delta as u32) == self.symbols.entry.0, "staged startup word does not branch to the linked entry")?;
                self.handoff = Some(machine.instructions());
            } else if address == self.symbols.entry.0 {
                require(self.handoff.is_some(), "published entry preceded successful load")?;
                self.first_entry.get_or_insert(machine.instructions());
                self.entries += 1;
            } else if address == self.symbols.halt.0 {
                require(self.handoff.is_none() && self.entries == 0,
                    "corrupt record did not stop at the load failure boundary")?;
                self.halted = true;
                return Ok(Advance::Halted);
            } else { return Err("unexpected fixed-image stop".into()); }
            self.hits = machine.traces.hits.len();
            self.arm(machine)?;
        }
        require(machine.instructions() == target, "fixed check ended before instruction bound")?;
        Ok(Advance::Complete)
    }

    fn complete(&mut self, machine: &mut Machine, count: u64) -> Result<(), String> {
        require(self.advance(machine, count)? == Advance::Complete, "positive fixed image halted")
    }

    fn key(&mut self, machine: &mut Machine, key: RouteKey) -> Result<(), String> {
        machine.press(key)?;
        self.complete(machine, 8_000_000)?;
        machine.release_key(key)?;
        self.complete(machine, 8_000_000)
    }
}

pub fn command(arguments: &[std::ffi::OsString]) -> Option<Result<(), String>> {
    if arguments.first().is_none_or(|argument| argument != "--fixed-image-check") { return None; }
    Some(drive(arguments))
}

fn drive(arguments: &[std::ffi::OsString]) -> Result<(), String> {
    require(arguments.len() == 14, "heap-observe --fixed-image-check success|corrupt --run RUN --update DAT --boot-rom ROM --first-symbols FILE --symbols FILE --output DIR")?;
    let scenario = match arguments[1].to_str() { Some("success") => Scenario::Success, Some("corrupt") => Scenario::Corrupt, _ => return Err("unknown fixed-image check scenario".into()) };
    let mut options = BTreeMap::new();
    for pair in arguments[2..].chunks_exact(2) {
        let flag = pair[0].to_str().ok_or("non-UTF8 fixed-image flag")?;
        require(["--run", "--update", "--boot-rom", "--first-symbols", "--symbols", "--output"].contains(&flag), "unknown fixed-image flag")?;
        require(options.insert(flag, PathBuf::from(&pair[1])).is_none(), "duplicate fixed-image flag")?;
    }
    let mut take = |flag| options.remove(flag).ok_or_else(|| format!("missing {flag}"));
    let run_bytes = read(&take("--run")?)?;
    let update = read(&take("--update")?)?;
    let rom = read(&take("--boot-rom")?)?;
    let first = read(&take("--first-symbols")?)?;
    let probe = read(&take("--symbols")?)?;
    let output = take("--output")?;
    std::fs::create_dir(&output).map_err(|error| error.to_string())?;
    let symbols = Symbols {
        load_limit: symbol_record(&first, "xm1_first_stage_fixed_image_load")?.1,
        load: symbol(&first, "xm1_first_stage_fixed_image_load")?, handoff: symbol(&first, "xm1_first_stage_handoff")?, halt: symbol(&first, "xm1_first_stage_fixed_image_halt")?,
        entry: symbol(&probe, "xm1_fixed_test_entry")?, data: symbol(&probe, "XM1_FIXED_TEST_DATA")?, count: symbol(&probe, "XM1_FIXED_TEST_COUNT")?, failures: symbol(&probe, "XM1_FIXED_TEST_FAILURES")?,
    };
    require([symbols.entry, symbols.data, symbols.count, symbols.failures].into_iter().all(|address| (IMAGE_BASE..IMAGE_LIMIT).contains(&address.0)), "probe symbols leave fixed range")?;
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let document = RunDocument::parse(&run_bytes)?;
    let maps = run::memory_maps(&document)?;
    let mut session = run::open(&document, &ValidatedUpdate(update.clone()), &rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let mut machine = Machine::new(document, session)?;
    require(machine.instructions() == 0, "fixed boot check must start from reset")?;
    let mut observer = Observer::begin(&mut machine, symbols, maps)?;
    let result = (|| -> Result<(), String> {
        match scenario {
            Scenario::Corrupt => { require(observer.advance(&mut machine, 80_000_000)? == Advance::Halted, "corrupt record did not halt within boot bound")?; }
            Scenario::Success => {
                observer.complete(&mut machine, 80_000_000)?;
                require(menu(&machine)? == Some([69, 52, 12]), "fixed boot initial setup differs")?;
                for key in std::iter::repeat_n(RouteKey::Up, 12).chain([RouteKey::Ok, RouteKey::Ok]) { observer.key(&mut machine, key)?; }
                observer.complete(&mut machine, 40_000_000)?;
                observer.key(&mut machine, RouteKey::Ok)?;
                observer.complete(&mut machine, 90_000_000)?;
                require(menu(&machine)? == Some([1, 5, 2]), "fixed boot did not reach shooting menu")?;
                observer.drain(&mut machine)?;
                let count = observer.value(&machine, observer.symbols.count)?;
                require(count as usize == observer.entries && count > 0 && observer.fixed_fetches > 0 && observer.owned_writes > 0
                    && observer.value(&machine, observer.symbols.failures)? == 0
                    && observer.value(&machine, observer.symbols.data)? == 0x13579bdf, "probe entry did not preserve in-place data/BSS across calls")?;
            }
        }
        Ok(())
    })();
    write(&output.join("fixed-image.json"), &serde_json::to_vec_pretty(&serde_json::json!({
        "status": if result.is_ok() { "passed" } else { "failed" }, "error": result.as_ref().err(), "scenario": arguments[1].to_string_lossy(),
        "dat_sha256": sha256(&update), "run_sha256": sha256(&run_bytes), "rom_sha256": sha256(&rom),
        "instructions": machine.session.board.instruction_counts(), "load_begin": observer.load_begin, "handoff": observer.handoff,
        "first_published_entry": observer.first_entry, "published_entry_calls": observer.entries, "fixed_instruction_fetches": observer.fixed_fetches,
        "owned_partition_writes": observer.owned_writes, "halted": observer.halted, "menu": menu(&machine)?
    })).map_err(|error| error.to_string())?)?;
    result
}
