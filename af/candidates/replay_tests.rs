use af_candidates::{Action, Candidate, Config, Controller, Direction, Event, Failure, Position,
    EvidenceCount, Percent, Score, SearchStep, StructuralFit, StructuralParameters};

const SINGLE: &str = include_str!("fixtures/single-peak.curve");
const MULTI: &str = include_str!("fixtures/two-depths.curve");
const EDGE: &str = include_str!("fixtures/edge-peak.curve");
const NOISY: &str = include_str!("fixtures/noisy-peak.curve");
const FLAT: &str = include_str!("fixtures/flat.curve");
const A0: Candidate = Candidate::StructuralReference(StructuralParameters::synthetic());
fn config(budget: u16) -> Config {
    Config::new(Position::new(0), Position::new(1000), 125, 10, budget, Score::new(100).unwrap()).unwrap()
}
fn score(fixture: &str, position: Position) -> Score {
    let points: Vec<(u16, i64)> = fixture.lines().skip(1).map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next().unwrap().parse().unwrap(), fields.next().unwrap().parse().unwrap())
    }).collect();
    let p = position.units();
    let pair = points.windows(2).find(|pair| pair[0].0 <= p && p <= pair[1].0).unwrap();
    let ((x, y), (right, value)) = (pair[0], pair[1]);
    Score::new((y + (value - y) * i64::from(p - x) / i64::from(right - x)) as u32).unwrap()
}
fn replay(candidate: Candidate, fixture: &str, budget: u16) -> (Action, u16) {
    assert!(fixture.starts_with("synthetic "));
    let mut controller = Controller::new(config(budget), candidate);
    let initial = fixture.lines().next().unwrap().split_whitespace().nth(3).unwrap().parse().unwrap();
    let mut action = controller.step(Event::Start { position: Position::new(initial) });
    for _ in 0..256 {
        match action {
            Action::Observe { request, position } => {
                assert!(position.units() <= 1000);
                action = controller.step(Event::Sample { request, position,
                    score: score(fixture, position), fresh: true, settled: true });
            }
            Action::Finish(_) | Action::Fail(_) => return (action, controller.samples()),
            Action::Wait => panic!("fresh settled synthetic sample unexpectedly waited"),
        }
    }
    panic!("controller exceeded finite replay");
}
#[test]
fn synthetic_single_peak_and_noise_focus_within_fine_step() {
    for candidate in [A0, Candidate::CoarseFine, Candidate::AdaptivePeak] {
        for fixture in [SINGLE, NOISY] {
            let (action, samples) = replay(candidate, fixture, 64);
            assert!(matches!(action, Action::Finish(p) if p.units().abs_diff(537) <= 10));
            assert!(samples <= 64);
        }
    }
}
#[test]
fn synthetic_coarse_search_selects_stronger_depth_and_edge() {
    for candidate in [Candidate::CoarseFine, Candidate::AdaptivePeak] {
        assert!(matches!(replay(candidate, MULTI, 64).0, Action::Finish(p) if p.units().abs_diff(730) <= 10));
        assert_eq!(replay(candidate, EDGE, 64).0, Action::Finish(Position::new(0)));
    }
}
#[test]
fn synthetic_flat_curve_and_exhausted_budget_refuse_focus() {
    for candidate in [A0, Candidate::CoarseFine, Candidate::AdaptivePeak] {
        assert_eq!(replay(candidate, FLAT, 64).0, Action::Fail(Failure::FlatCurve));
        assert_eq!(replay(candidate, SINGLE, 3).0, Action::Fail(Failure::Budget));
    }
}
#[test]
fn missing_stale_and_unsettled_observations_do_not_advance_focus() {
    let mut controller = Controller::new(config(64), Candidate::AdaptivePeak);
    let Action::Observe { request, position } = controller.step(Event::Start { position: Position::new(500) }) else { panic!() };
    assert_eq!(controller.step(Event::Missing), Action::Wait);
    for (fresh, settled) in [(false, true), (true, false)] {
        assert_eq!(controller.step(Event::Sample { request, position, score: Score::new(1).unwrap(), fresh, settled }), Action::Wait);
    }
    assert_eq!(controller.samples(), 0);
    assert_eq!(controller.step(Event::Cancel), Action::Fail(Failure::Cancelled));
    assert_eq!(controller.step(Event::Missing), Action::Fail(Failure::Cancelled));
}
#[test]
fn repeated_missing_observations_fail_in_finite_time() {
    let mut controller = Controller::new(config(64), Candidate::CoarseFine);
    controller.step(Event::Start { position: Position::new(500) });
    let mut result = Action::Wait;
    for _ in 0..256 { result = controller.step(Event::Missing); }
    assert_eq!(result, Action::Fail(Failure::Budget));
}
#[test]
fn wrong_position_and_changing_final_measurement_refuse_focus() {
    let mut controller = Controller::new(config(64), Candidate::CoarseFine);
    let Action::Observe { request, .. } = controller.step(Event::Start { position: Position::new(500) }) else { panic!() };
    assert_eq!(controller.step(Event::Sample { request, position: Position::new(1),
        score: Score::new(1).unwrap(), fresh: true, settled: true }), Action::Fail(Failure::InvalidObservation));
    let mut controller = Controller::new(config(64), Candidate::AdaptivePeak);
    let mut action = controller.step(Event::Start { position: Position::new(500) });
    let mut measured = Vec::new();
    while let Action::Observe { request, position } = action {
        let repeated = measured.contains(&position);
        measured.push(position);
        action = controller.step(Event::Sample { request, position,
            score: if repeated { Score::new(0).unwrap() } else { score(SINGLE, position) },
            fresh: true, settled: true });
    }
    assert_eq!(action, Action::Fail(Failure::InvalidObservation));
}

#[test]
fn structural_reference_stays_on_the_first_depth_and_reaches_a_boundary_peak() {
    assert!(matches!(replay(A0, MULTI, 64).0, Action::Finish(p) if p.units().abs_diff(230) <= 10));
    assert_eq!(replay(A0, EDGE, 64).0, Action::Finish(Position::new(0)));
}

#[test]
fn structural_reference_accepts_either_direction_and_linear_side_refinement() {
    let parameters = StructuralParameters::new(SearchStep::new(50).unwrap(), SearchStep::new(50).unwrap(),
        EvidenceCount::new(1).unwrap(), EvidenceCount::new(2).unwrap(), Percent::new(1).unwrap(),
        Score::new(100).unwrap(), Direction::Decreasing, StructuralFit::LinearSides);
    assert!(matches!(replay(Candidate::StructuralReference(parameters), SINGLE, 64).0,
        Action::Finish(p) if p.units().abs_diff(537) <= 10));
}

#[test]
fn structural_reference_uses_initial_position_and_rejects_outside_bounds() {
    let mut controller = Controller::new(config(64), A0);
    assert!(matches!(controller.step(Event::Start { position: Position::new(700) }),
        Action::Observe { position, .. } if position == Position::new(700)));
    let mut controller = Controller::new(config(64), A0);
    assert_eq!(controller.step(Event::Start { position: Position::new(1001) }),
        Action::Fail(Failure::InvalidObservation));
}
