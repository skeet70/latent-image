//! `FUJI_NATIVE_V1` wire on stdin, one observation line on stdout. Exit 0
//! means `returned`, 1 any other observation, 2 an I/O failure.

use fujimod_xm1::native_case::{self, MAX_WIRE_BYTES};
use std::io::{Read, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut wire = Vec::new();
    if let Err(error) = std::io::stdin().lock().take(MAX_WIRE_BYTES as u64 + 1).read_to_end(&mut wire) {
        eprintln!("native-case: reading stdin failed: {error}");
        return ExitCode::from(2);
    }
    let outcome = native_case::run(&wire);
    let mut stdout = std::io::stdout().lock();
    if let Err(error) = stdout.write_all(&outcome.json).and_then(|()| stdout.flush()) {
        eprintln!("native-case: writing stdout failed: {error}");
        return ExitCode::from(2);
    }
    ExitCode::from(if outcome.returned { 0 } else { 1 })
}
