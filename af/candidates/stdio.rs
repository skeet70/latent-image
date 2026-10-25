use af_candidates::{
    Action, Candidate, ChannelMean, Config, Controller, Event, FocusStatus, LibcameraMode, Position,
    RgbMeans, SceneStatistics, Score, StructuralParameters,
};
use std::io::{self, BufRead, Write};
use std::str::SplitWhitespace;

fn score(text: Option<&str>) -> Result<Score, Box<dyn std::error::Error>> {
    Score::new(text.ok_or("missing score")?.parse()?)
        .ok_or_else(|| "score outside bounded domain".into())
}

fn channel(text: Option<&str>) -> Result<ChannelMean, Box<dyn std::error::Error>> {
    ChannelMean::new(text.ok_or("missing channel mean")?.parse()?)
        .ok_or_else(|| "channel mean outside bounded domain".into())
}

fn scene(mut fields: SplitWhitespace<'_>) -> Result<SceneStatistics, Box<dyn std::error::Error>> {
    let contrast = score(fields.next())?;
    let red = channel(fields.next())?;
    let green = channel(fields.next())?;
    let blue = channel(fields.next())?;
    if fields.next().is_some() {
        return Err("unexpected scene fields".into());
    }
    Ok(SceneStatistics::new(contrast, RgbMeans::new(red, green, blue)))
}

fn response(
    action: Action,
    candidate: Candidate,
    line: &str,
) -> Result<Event, Box<dyn std::error::Error>> {
    match action {
        Action::WaitFrame { request, position } => {
            if line.trim_end_matches(['\r', '\n']) != "FRAME" {
                return Err("expected FRAME".into());
            }
            Ok(Event::Frame { request, position, fresh: true, settled: true })
        }
        Action::Observe { request, position } | Action::Monitor { request, position, .. } => {
            let mut fields = line.split_whitespace();
            match fields.next() {
                Some("SCENE") => Ok(Event::SceneSample {
                    request, position, statistics: scene(fields)?, fresh: true, settled: true,
                }),
                text if matches!(action, Action::Observe { .. })
                    && !matches!(candidate, Candidate::LibcameraContrast(LibcameraMode::Continuous)) =>
                {
                    let score = score(text)?;
                    if fields.next().is_some() {
                        return Err("expected one score".into());
                    }
                    Ok(Event::Sample { request, position, score, fresh: true, settled: true })
                }
                _ => Err("expected SCENE contrast red green blue".into()),
            }
        }
        _ => Err("unexpected wait".into()),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Err("missing configuration".into());
    }
    let mut fields = line.split_whitespace();
    if fields.next() != Some("CANDIDATE2") {
        return Err("invalid configuration".into());
    }
    let candidate = match fields.next() {
        Some("A0-structural") => Candidate::StructuralReference(StructuralParameters::run3_viltrox()),
        Some("A1") => Candidate::CoarseFine,
        Some("A2") => Candidate::AdaptivePeak,
        Some("A3-libcamera") => Candidate::LibcameraContrast(LibcameraMode::Triggered),
        Some("A3-libcamera-continuous") => Candidate::LibcameraContrast(LibcameraMode::Continuous),
        _ => return Err("unknown candidate".into()),
    };
    let high = Position::new(fields.next().ok_or("missing range maximum")?.parse()?);
    let initial = Position::new(fields.next().ok_or("missing initial position")?.parse()?);
    if fields.next().is_some() {
        return Err("invalid configuration".into());
    }
    let config = Config::new(Position::new(0), high, 125, 10, 64,
        Score::new(100).ok_or("score")?).ok_or("unsupported range")?;
    let mut controller = Controller::new(config, candidate);
    let mut action = controller.step(Event::Start { position: initial });
    loop {
        match action {
            Action::Observe { request, position } => println!("OBSERVE {} {}", request.units(), position.units()),
            Action::WaitFrame { request, position } => println!("WAIT_FRAME {} {}", request.units(), position.units()),
            Action::Monitor { request, position, status } => {
                let status = match status {
                    FocusStatus::Focused => "focused",
                    FocusStatus::Failed => "failed",
                };
                println!("MONITOR {} {} {status}", request.units(), position.units());
            }
            Action::Wait => println!("WAIT"),
            Action::Finish(position) => { println!("FINISH {} {}", position.units(), controller.samples()); break; }
            Action::Fail(reason) => { println!("FAIL {reason:?} {}", controller.samples()); break; }
        }
        io::stdout().flush()?;
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Err("missing observation".into());
        }
        action = controller.step(response(action, candidate, &line)?);
    }
    Ok(())
}
