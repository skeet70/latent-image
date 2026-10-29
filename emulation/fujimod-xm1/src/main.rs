//! Cold-boots X-M1 1.50 on FujiMod's EXR II runtime and answers the
//! interactive protocol of `tools/probe_setup_menus.py`, so
//! `Emulation.FlashBootRoute.judgeProbeOutput` judges its transcript.
use fujimod_xm1::guest::RouteKey;
use fujimod_xm1::machine::{KeyPulse, Machine};
use fujimod_xm1::run::{self, RunDocument, ValidatedUpdate};
use fujimod_xm1::witness;
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

const USAGE: &str = "Usage: fujimod-xm1 --run RUN.json --update DAT --boot-rom ROM --output DIR [--witness CONTRACT.json]\n\
The caller validates DAT as X-M1 1.50-compatible. Commands on stdin:\n\
  key NAME HOLD RELEASE | run COUNT | quit\n";

/// Largest single continuation the probe accepts.
const MAX_PHASE: u64 = 15_000_000;

struct Arguments {
    run: PathBuf,
    update: PathBuf,
    boot_rom: PathBuf,
    output: PathBuf,
    witness: Option<PathBuf>,
}

fn arguments() -> Result<Arguments, String> {
    let mut run = None;
    let mut update = None;
    let mut boot_rom = None;
    let mut output = None;
    let mut witness = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().map(PathBuf::from).ok_or_else(|| USAGE.to_owned())?;
        match flag.to_str() {
            Some("--run") => run = Some(value),
            Some("--update") => update = Some(value),
            Some("--boot-rom") => boot_rom = Some(value),
            Some("--output") => output = Some(value),
            Some("--witness") => witness = Some(value),
            _ => return Err(USAGE.into()),
        }
    }
    match (run, update, boot_rom, output) {
        (Some(run), Some(update), Some(boot_rom), Some(output)) => {
            Ok(Arguments { run, update, boot_rom, output, witness })
        }
        _ => Err(USAGE.into()),
    }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

struct Driver {
    machine: Machine,
    output: PathBuf,
    frames: usize,
}

/// One line per stop, without the separator words the transcript parser splits on.
fn one_line(text: &str) -> String {
    text.replace('\n', " ").replace(" error=", " error:").replace(" failure=", " failure:")
}

impl Driver {
    fn print_state(&mut self, out: &mut impl Write) -> Result<(), String> {
        let menu = self.machine.menu();
        let frame = match self.machine.frame() {
            Ok(Some(png)) => {
                self.frames += 1;
                let path = self.output.join(format!("frame-{:03}.png", self.frames));
                std::fs::write(&path, &png).map_err(|e| format!("{}: {e}", path.display()))?;
                format!("{:x}", Sha256::digest(&png))
            }
            Ok(None) => "None".into(),
            Err(error) => {
                eprintln!("frame: {error}");
                "None".into()
            }
        };
        let failure = self.machine.failure.as_deref().map(one_line).unwrap_or_else(|| "None".into());
        let menu = match menu {
            Ok(Some(menu)) => format!("id={} setting={} cursor={}", menu.menu_id, menu.setting_id, menu.cursor),
            Ok(None) => "menu=None".into(),
            Err(error) => format!("menu-error={}", one_line(&error).replace(' ', "_")),
        };
        writeln!(
            out,
            "instructions={} failure={failure} error=None\n  ticks={} {menu}\n  frame={frame} output={}",
            self.machine.instructions(),
            self.machine.session.board.now(),
            self.output.display()
        )
        .map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())
    }
}

fn command(driver: &mut Driver, line: &str) -> Result<bool, String> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let count = |text: &str| -> Result<u64, String> {
        let value: u64 = text.parse().map_err(|_| format!("not an instruction count: {text}"))?;
        if value > MAX_PHASE {
            return Err(format!("continuation must be 0..{MAX_PHASE}"));
        }
        Ok(value)
    };
    match fields.as_slice() {
        ["quit"] => Ok(false),
        ["run", limit] => {
            let limit = count(limit)?;
            driver.machine.run(limit);
            Ok(true)
        }
        ["key", name, hold, release] => {
            let key = RouteKey::parse(name)?;
            driver.machine.key(key, KeyPulse { hold: count(hold)?, release: count(release)? })?;
            Ok(true)
        }
        _ => Err(format!("unknown command: {line}")),
    }
}

fn drive() -> Result<(), String> {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let arguments = arguments()?;
    let document = RunDocument::parse(&read(&arguments.run)?)?;
    let update = ValidatedUpdate(read(&arguments.update)?);
    let boot_rom = read(&arguments.boot_rom)?;
    std::fs::create_dir_all(&arguments.output).map_err(|e| format!("{}: {e}", arguments.output.display()))?;
    let mut session = run::open(&document, &update, &boot_rom)?;
    session.board.set_execution_policy(document.execution_policy.into())?;
    let boot = document.boot_instructions;
    let machine = Machine::new(document, session)?;
    let output = arguments.output;
    if let Some(contract_path) = &arguments.witness {
        let contract = witness::WitnessContract::parse(&read(contract_path)?)?;
        return witness::run(machine, contract, &output);
    }
    let mut driver = Driver { machine, output, frames: 0 };
    driver.machine.run(boot);
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    driver.print_state(&mut out)?;
    writeln!(out, "Commands: key NAME HOLD RELEASE | run COUNT | quit").map_err(|e| e.to_string())?;
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        match command(&mut driver, &line) {
            Ok(false) => break,
            Ok(true) => driver.print_state(&mut out)?,
            Err(error) => writeln!(out, "{}", one_line(&error)).map_err(|e| e.to_string())?,
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match drive() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
