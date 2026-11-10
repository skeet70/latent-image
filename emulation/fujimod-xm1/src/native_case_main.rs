//! `FUJI_NATIVE_V1` or `FUJI_NATIVE_SCRIPT_V1` on stdin, one observation line on stdout. Exit 0
//! means `returned`, 1 any other observation, 2 an I/O failure.
//!
//! With the sole argument `--batch`: a `FUJI_NATIVE_BATCH_V1` wire on stdin and
//! one frame per case on stdout (`native_case::run_batch`). Exit 0 means every
//! case ran, 1 a refused batch framing, 2 an I/O failure.

use fujimod_xm1::native_case::{self, MAX_BATCH_WIRE_BYTES, MAX_WIRE_BYTES};
use std::io::{Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let batch = std::env::args_os().skip(1).eq(["--batch"]);
    let limit = if batch { MAX_BATCH_WIRE_BYTES } else { MAX_WIRE_BYTES };
    let mut wire = Vec::new();
    if let Err(error) = std::io::stdin().lock().take(limit as u64 + 1).read_to_end(&mut wire) {
        eprintln!("native-case: reading stdin failed: {error}");
        return ExitCode::from(2);
    }
    let mut stdout = std::io::stdout().lock();
    if batch {
        return match native_case::run_batch(&wire, &mut stdout) {
            Ok(ran) => ExitCode::from(if ran { 0 } else { 1 }),
            Err(error) => {
                eprintln!("native-case: writing stdout failed: {error}");
                ExitCode::from(2)
            }
        };
    }
    let outcome = native_case::run(&wire);
    if let Err(error) = stdout.write_all(&outcome.json).and_then(|()| stdout.flush()) {
        eprintln!("native-case: writing stdout failed: {error}");
        return ExitCode::from(2);
    }
    ExitCode::from(if outcome.returned { 0 } else { 1 })
}
