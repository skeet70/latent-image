use af_candidates::{Action, Candidate, Config, Controller, Event, Position, Score, StructuralParameters};
use std::io::{self, BufRead, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let config = lines.next().ok_or("missing configuration")??;
    let fields: Vec<_> = config.split_whitespace().collect();
    if fields.len() != 4 || fields[0] != "CANDIDATE2" { return Err("invalid configuration".into()); }
    let candidate = match fields[1] {
        "A0-structural" => Candidate::StructuralReference(StructuralParameters::synthetic()),
        "A1" => Candidate::CoarseFine,
        "A2" => Candidate::AdaptivePeak,
        _ => return Err("unknown candidate".into()),
    };
    let high: u16 = fields[2].parse()?;
    let initial = Position::new(fields[3].parse()?);
    let config = Config::new(Position::new(0), Position::new(high), 125, 10, 64,
        Score::new(100).ok_or("score")?).ok_or("unsupported range")?;
    let mut controller = Controller::new(config, candidate);
    let mut action = controller.step(Event::Start { position: initial });
    loop {
        match action {
            Action::Observe { request, position } => println!("OBSERVE {} {}", request.units(), position.units()),
            Action::Wait => println!("WAIT"),
            Action::Finish(position) => { println!("FINISH {} {}", position.units(), controller.samples()); break; }
            Action::Fail(reason) => { println!("FAIL {reason:?} {}", controller.samples()); break; }
        }
        io::stdout().flush()?;
        let line = lines.next().ok_or("missing observation")??;
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 1 { return Err("expected one score".into()); }
        let score = Score::new(fields[0].parse()?).ok_or("score outside bounded domain")?;
        action = match action {
            Action::Observe { request, position } => controller.step(Event::Sample {
                request, position, score, fresh: true, settled: true,
            }),
            _ => return Err("unexpected wait".into()),
        };
    }
    Ok(())
}
