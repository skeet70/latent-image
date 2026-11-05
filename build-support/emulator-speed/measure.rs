use std::{env, ffi::OsString, fs::{self, File}, path::PathBuf, process::{Command, ExitCode, Stdio}};

const USAGE: &str = "measure --bridge FILE --run FILE --boot-rom FILE --installed-dat FILE --adjustment-nand FILE --checkpoints DIR --output NEW_DIR [--time FILE] [--uptime FILE] [--resume-input FILE]";
const BOOT: &str = "{\"command\":\"start\",\"installed\":null}\n{\"command\":\"finish\"}\n";
const RESUME: &str = "{\"command\":\"start\",\"installed\":null}\n{\"command\":\"idle\",\"instructions\":40000000}\n{\"command\":\"finish\"}\n";

struct Arguments {
    bridge: PathBuf, run: PathBuf, rom: PathBuf, dat: PathBuf, nand: PathBuf,
    checkpoints: PathBuf, output: PathBuf, time: OsString, uptime: OsString,
    resume: Option<PathBuf>,
}

fn arguments() -> Result<Arguments, String> {
    let mut values = std::collections::BTreeMap::new();
    let mut args = env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let flag = flag.into_string().map_err(|_| USAGE)?;
        if !["--bridge", "--run", "--boot-rom", "--installed-dat", "--adjustment-nand", "--checkpoints", "--output", "--time", "--uptime", "--resume-input"].contains(&flag.as_str()) {
            return Err(USAGE.into());
        }
        if values.insert(flag, args.next().ok_or(USAGE)?).is_some() { return Err(USAGE.into()); }
    }
    let time = values.remove("--time").unwrap_or_else(|| "time".into());
    let uptime = values.remove("--uptime").unwrap_or_else(|| "uptime".into());
    let resume = values.remove("--resume-input").map(PathBuf::from);
    let mut take = |flag| values.remove(flag).map(PathBuf::from).ok_or_else(|| USAGE.to_owned());
    Ok(Arguments { bridge: take("--bridge")?, run: take("--run")?, rom: take("--boot-rom")?,
        dat: take("--installed-dat")?, nand: take("--adjustment-nand")?, checkpoints: take("--checkpoints")?,
        output: take("--output")?, time, uptime, resume })
}

#[derive(Clone, Copy)]
enum Scenario { Cold, Restored, Resumed }
impl Scenario {
    fn name(self) -> &'static str { match self { Self::Cold => "cold", Self::Restored => "restored", Self::Resumed => "resumed" } }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = arguments()?;
    fs::create_dir(&args.output)?;
    fs::write(args.output.join("boot-input.jsonl"), BOOT)?;
    let resumed = args.resume.as_ref().map(fs::read).transpose()?.unwrap_or_else(|| RESUME.as_bytes().to_vec());
    fs::write(args.output.join("resume-input.jsonl"), resumed)?;
    for scenario in [Scenario::Cold, Scenario::Restored, Scenario::Resumed] {
        let name = scenario.name();
        let load = Command::new(&args.uptime).output()?;
        if !load.status.success() { return Err("uptime failed".into()); }
        fs::write(args.output.join(format!("{name}.uptime.txt")), &load.stdout)?;
        let mut command = Command::new(&args.time);
        command.args(["-v", "-o"]).arg(args.output.join(format!("{name}.time.txt")))
            .arg(&args.bridge).arg("--run").arg(&args.run).arg("--boot-rom").arg(&args.rom)
            .arg("--installed-dat").arg(&args.dat).arg("--adjustment-nand").arg(&args.nand)
            .arg("--ram-push-checkpoints").arg(&args.checkpoints).arg("--output").arg(args.output.join(name));
        if matches!(scenario, Scenario::Cold) { command.arg("--fresh-boot"); }
        let input = if matches!(scenario, Scenario::Resumed) { "resume-input.jsonl" } else { "boot-input.jsonl" };
        command.stdin(Stdio::from(File::open(args.output.join(input))?))
            .stdout(Stdio::from(File::create(args.output.join(format!("{name}.stdout.jsonl")))?))
            .stderr(Stdio::from(File::create(args.output.join(format!("{name}.stderr.log")))?));
        fs::write(args.output.join(format!("{name}.command.txt")), format!("{command:?}\n"))?;
        let status = command.status()?;
        print!("{name}: {}", String::from_utf8_lossy(&load.stdout));
        println!("{}", fs::read_to_string(args.output.join(format!("{name}.time.txt")))?);
        if !status.success() { return Err(format!("{name} failed: {status}").into()); }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("{error}"); ExitCode::FAILURE } }
}
