use af_candidates::{
    Action, Candidate, ChannelMean, Controller, Event, Failure, FocusStatus, LibcameraMode,
    Position, RgbMeans, SceneStatistics, Score,
};

use super::{config, replay, score, EDGE, FLAT, MULTI, NOISY, SINGLE};

// Synthetic contrast frames derive only from this repository's original .curve fixtures.
#[derive(Clone, Copy, Debug)]
enum SyntheticChange {
    Unchanged,
    ContrastDrop,
    RedDrop,
    GreenDrop,
    BlueDrop,
    RedRise,
    GreenRise,
    BlueRise,
}

fn synthetic_statistics(fixture: &str, position: Position, change: SyntheticChange) -> SceneStatistics {
    assert!(fixture.starts_with("synthetic "));
    let contrast = score(fixture, position);
    let contrast = match change {
        SyntheticChange::ContrastDrop => Score::new(contrast.units() / 2).unwrap(),
        _ => contrast,
    };
    let (red, green, blue) = match change {
        SyntheticChange::RedDrop => (500, 1000, 1000),
        SyntheticChange::GreenDrop => (1000, 500, 1000),
        SyntheticChange::BlueDrop => (1000, 1000, 500),
        SyntheticChange::RedRise => (2000, 1000, 1000),
        SyntheticChange::GreenRise => (1000, 2000, 1000),
        SyntheticChange::BlueRise => (1000, 1000, 2000),
        _ => (1000, 1000, 1000),
    };
    SceneStatistics::new(contrast, RgbMeans::new(
        ChannelMean::new(red).unwrap(),
        ChannelMean::new(green).unwrap(),
        ChannelMean::new(blue).unwrap(),
    ))
}

fn synthetic_response(controller: &mut Controller, action: Action, fixture: &str, mode: LibcameraMode) -> Action {
    match action {
        Action::WaitFrame { request, position } => controller.step(Event::Frame {
            request, position, fresh: true, settled: true,
        }),
        Action::Observe { request, position } => match mode {
            LibcameraMode::Triggered => controller.step(Event::Sample {
                request, position, score: score(fixture, position), fresh: true, settled: true,
            }),
            LibcameraMode::Continuous => controller.step(Event::SceneSample {
                request, position, statistics: synthetic_statistics(fixture, position, SyntheticChange::Unchanged),
                fresh: true, settled: true,
            }),
        },
        _ => panic!("expected a synthetic scan frame, got {action:?}"),
    }
}

fn synthetic_continuous_monitor(fixture: &str, initial: Position) -> (Controller, Action) {
    let mut controller = Controller::new(config(64), Candidate::LibcameraContrast(LibcameraMode::Continuous));
    let mut action = controller.step(Event::Start { position: initial });
    for _ in 0..256 {
        if matches!(action, Action::Monitor { .. }) {
            return (controller, action);
        }
        action = synthetic_response(&mut controller, action, fixture, LibcameraMode::Continuous);
    }
    panic!("synthetic continuous search did not settle within the host bound");
}

fn synthetic_monitor_response(controller: &mut Controller, action: Action, change: SyntheticChange) -> Action {
    let Action::Monitor { request, position, .. } = action else {
        panic!("expected synthetic monitoring, got {action:?}");
    };
    controller.step(Event::SceneSample {
        request, position, statistics: synthetic_statistics(SINGLE, position, change),
        fresh: true, settled: true,
    })
}

fn assert_focused_monitor(action: Action) {
    assert!(matches!(action, Action::Monitor { status: FocusStatus::Focused, position, .. }
        if position.units().abs_diff(537) <= 10), "{action:?}");
}

fn assert_search_restarted(action: Action) {
    assert!(matches!(action, Action::WaitFrame { .. } | Action::Observe { .. }), "{action:?}");
}

#[test]
fn synthetic_libcamera_three_good_fixtures_focus_and_two_low_range_fixtures_refuse() {
    let candidate = Candidate::LibcameraContrast(LibcameraMode::Triggered);
    for (fixture, peak) in [(SINGLE, 537), (NOISY, 537), (EDGE, 0)] {
        let (action, samples) = replay(candidate, fixture, 64);
        assert!(matches!(action, Action::Finish(position) if position.units().abs_diff(peak) <= 10), "{action:?}");
        assert!(samples <= 64);
    }
    for fixture in [FLAT, MULTI] {
        assert_eq!(replay(candidate, fixture, 64).0, Action::Fail(Failure::LowConfidence));
    }
}

#[test]
fn synthetic_libcamera_settling_requires_fresh_frames_and_cannot_be_bypassed_by_samples() {
    let mut controller = Controller::new(config(64), Candidate::LibcameraContrast(LibcameraMode::Triggered));
    let mut action = controller.step(Event::Start { position: Position::new(1000) });
    let mut completed_frames = 0;
    let mut previous_position = Position::new(1000);
    let mut completed_requests = Vec::new();
    while let Action::WaitFrame { request, position } = action {
        assert!(completed_frames < 32);
        assert!(position.units().abs_diff(previous_position.units()) <= 167);
        assert!(!completed_requests.contains(&request));
        assert_eq!(controller.step(Event::Missing), Action::Wait);
        assert_eq!(controller.step(Event::Sample {
            request, position, score: score(SINGLE, position), fresh: true, settled: true,
        }), Action::Wait);
        for (fresh, settled) in [(false, true), (true, false)] {
            assert_eq!(controller.step(Event::Frame { request, position, fresh, settled }), Action::Wait);
        }
        assert_eq!(controller.samples(), 0);
        completed_requests.push(request);
        action = controller.step(Event::Frame { request, position, fresh: true, settled: true });
        previous_position = position;
        completed_frames += 1;
    }
    assert!(completed_frames >= 5);
    let Action::Observe { request, position } = action else { panic!("{action:?}") };
    assert_eq!(position, Position::new(0));
    assert!(!completed_requests.contains(&request));
    for (fresh, settled) in [(false, true), (true, false)] {
        assert_eq!(controller.step(Event::Sample {
            request, position, score: score(SINGLE, position), fresh, settled,
        }), Action::Wait);
    }
    assert_eq!(controller.samples(), 0);
    action = controller.step(Event::Sample {
        request, position, score: score(SINGLE, position), fresh: true, settled: true,
    });
    assert_eq!(controller.samples(), 1);
    let mut settled_at_next_position = 0;
    let mut next_position = None;
    while let Action::WaitFrame { request, position } = action {
        assert!(settled_at_next_position < 16);
        assert!(!completed_requests.contains(&request));
        completed_requests.push(request);
        if next_position == Some(position) {
            settled_at_next_position += 1;
        } else {
            next_position = Some(position);
            settled_at_next_position = 1;
        }
        assert_eq!(controller.step(Event::Sample {
            request, position, score: score(SINGLE, position), fresh: true, settled: true,
        }), Action::Wait);
        assert_eq!(controller.samples(), 1);
        action = controller.step(Event::Frame { request, position, fresh: true, settled: true });
    }
    assert!(settled_at_next_position >= 4);
    assert!(matches!(action, Action::Observe { position, .. } if Some(position) == next_position));
    assert_eq!(controller.samples(), 1);
}

#[test]
fn synthetic_libcamera_continuous_scans_require_scene_statistics() {
    let mut controller = Controller::new(config(64), Candidate::LibcameraContrast(LibcameraMode::Continuous));
    let mut action = controller.step(Event::Start { position: Position::new(0) });
    for _ in 0..32 {
        if matches!(action, Action::Observe { .. }) { break; }
        action = synthetic_response(&mut controller, action, SINGLE, LibcameraMode::Continuous);
    }
    let Action::Observe { request, position } = action else { panic!("{action:?}") };
    assert_eq!(controller.step(Event::Sample {
        request, position, score: score(SINGLE, position), fresh: true, settled: true,
    }), Action::Wait);
    assert_eq!(controller.samples(), 0);
    for (fresh, settled) in [(false, true), (true, false)] {
        assert_eq!(controller.step(Event::SceneSample {
            request, position, statistics: synthetic_statistics(SINGLE, position, SyntheticChange::Unchanged),
            fresh, settled,
        }), Action::Wait);
    }
    assert_eq!(controller.samples(), 0);
    synthetic_response(&mut controller, action, SINGLE, LibcameraMode::Continuous);
    assert_eq!(controller.samples(), 1);
}

#[test]
fn synthetic_libcamera_cancellation_is_terminal_during_wait_observation_and_monitoring() {
    for mode in [LibcameraMode::Triggered, LibcameraMode::Continuous] {
        for cancel_at_observation in [false, true] {
            let mut controller = Controller::new(config(64), Candidate::LibcameraContrast(mode));
            let mut action = controller.step(Event::Start { position: Position::new(0) });
            if cancel_at_observation {
                for _ in 0..32 {
                    if matches!(action, Action::Observe { .. }) { break; }
                    action = synthetic_response(&mut controller, action, SINGLE, mode);
                }
                assert!(matches!(action, Action::Observe { .. }));
            } else {
                assert!(matches!(action, Action::WaitFrame { .. }));
            }
            assert_eq!(controller.step(Event::Cancel), Action::Fail(Failure::Cancelled));
            assert_eq!(controller.step(Event::Missing), Action::Fail(Failure::Cancelled));
        }
    }
    let (mut controller, _) = synthetic_continuous_monitor(SINGLE, Position::new(0));
    assert_eq!(controller.step(Event::Cancel), Action::Fail(Failure::Cancelled));
    assert_eq!(controller.step(Event::Missing), Action::Fail(Failure::Cancelled));
}

#[test]
fn synthetic_libcamera_observation_and_missing_frame_budgets_are_bounded() {
    assert_eq!(replay(Candidate::LibcameraContrast(LibcameraMode::Triggered), SINGLE, 3),
        (Action::Fail(Failure::Budget), 3));
    for mode in [LibcameraMode::Triggered, LibcameraMode::Continuous] {
        let mut controller = Controller::new(config(64), Candidate::LibcameraContrast(mode));
        controller.step(Event::Start { position: Position::new(0) });
        let mut action = Action::Wait;
        for _ in 0..256 { action = controller.step(Event::Missing); }
        assert_eq!(action, Action::Fail(Failure::Budget));
        assert_eq!(controller.samples(), 0);
    }
}

#[test]
fn synthetic_libcamera_changed_final_contrast_refuses_confidence() {
    let candidate = Candidate::LibcameraContrast(LibcameraMode::Triggered);
    let (baseline, samples) = replay(candidate, SINGLE, 64);
    assert!(matches!(baseline, Action::Finish(_)));
    let mut controller = Controller::new(config(64), candidate);
    let mut action = controller.step(Event::Start { position: Position::new(500) });
    for _ in 0..256 {
        if let Action::Observe { request, position } = action {
            if controller.samples() + 1 == samples {
                assert!(matches!(baseline, Action::Finish(final_position) if final_position == position));
                action = controller.step(Event::Sample {
                    request, position, score: Score::new(0).unwrap(), fresh: true, settled: true,
                });
                assert_eq!(action, Action::Fail(Failure::LowConfidence));
                return;
            }
        }
        action = synthetic_response(&mut controller, action, SINGLE, LibcameraMode::Triggered);
    }
    panic!("synthetic final contrast was not requested within the host bound");
}

#[test]
fn synthetic_libcamera_stable_scene_never_retriggers_and_monitoring_consumes_budget() {
    let (mut controller, mut action) = synthetic_continuous_monitor(SINGLE, Position::new(0));
    assert_focused_monitor(action);
    for _ in 0..64 {
        let before = controller.samples();
        action = synthetic_monitor_response(&mut controller, action, SyntheticChange::Unchanged);
        assert_eq!(controller.samples(), before + 1);
        if action == Action::Fail(Failure::Budget) {
            assert_eq!(controller.samples(), 64);
            return;
        }
        assert_focused_monitor(action);
    }
    panic!("synthetic monitoring exceeded the session observation budget");
}

#[test]
fn synthetic_libcamera_contrast_and_rgb_changes_retrigger_only_after_ten_fresh_stable_frames() {
    for change in [SyntheticChange::ContrastDrop, SyntheticChange::RedDrop, SyntheticChange::GreenDrop,
        SyntheticChange::BlueDrop, SyntheticChange::RedRise, SyntheticChange::GreenRise, SyntheticChange::BlueRise] {
        let (mut controller, mut action) = synthetic_continuous_monitor(SINGLE, Position::new(0));
        for _ in 0..9 {
            let Action::Monitor { request, position, .. } = action else { panic!("{action:?}") };
            let before = controller.samples();
            for (fresh, settled) in [(false, true), (true, false)] {
                assert_eq!(controller.step(Event::SceneSample {
                    request, position, statistics: synthetic_statistics(SINGLE, position, change), fresh, settled,
                }), Action::Wait);
            }
            assert_eq!(controller.step(Event::Sample {
                request, position, score: score(SINGLE, position), fresh: true, settled: true,
            }), Action::Wait);
            assert_eq!(controller.samples(), before);
            action = synthetic_monitor_response(&mut controller, action, change);
            assert_focused_monitor(action);
        }
        action = synthetic_monitor_response(&mut controller, action, change);
        assert_search_restarted(action);
    }
}

#[test]
fn synthetic_libcamera_another_scene_change_resets_the_stability_window() {
    let (mut controller, mut action) = synthetic_continuous_monitor(SINGLE, Position::new(0));
    for _ in 0..9 {
        action = synthetic_monitor_response(&mut controller, action, SyntheticChange::ContrastDrop);
        assert_focused_monitor(action);
    }
    for _ in 0..9 {
        action = synthetic_monitor_response(&mut controller, action, SyntheticChange::Unchanged);
        assert_focused_monitor(action);
    }
    action = synthetic_monitor_response(&mut controller, action, SyntheticChange::Unchanged);
    assert_search_restarted(action);
}

#[test]
fn synthetic_libcamera_retrigger_does_not_reset_the_session_budget() {
    let (mut controller, mut action) = synthetic_continuous_monitor(SINGLE, Position::new(0));
    for _ in 0..10 {
        action = synthetic_monitor_response(&mut controller, action, SyntheticChange::ContrastDrop);
    }
    assert_search_restarted(action);
    let before_retrigger = controller.samples();
    for _ in 0..256 {
        action = match action {
            Action::WaitFrame { .. } | Action::Observe { .. } =>
                synthetic_response(&mut controller, action, SINGLE, LibcameraMode::Continuous),
            Action::Monitor { .. } =>
                synthetic_monitor_response(&mut controller, action, SyntheticChange::Unchanged),
            Action::Fail(Failure::Budget) => {
                assert_eq!(controller.samples(), 64);
                assert!(controller.samples() > before_retrigger);
                return;
            }
            _ => panic!("unexpected synthetic retrigger result {action:?}"),
        };
    }
    panic!("synthetic retrigger exceeded the session host bound");
}

#[test]
fn synthetic_libcamera_interior_start_searches_both_sides_before_reporting_focus() {
    let mut controller = Controller::new(config(64), Candidate::LibcameraContrast(LibcameraMode::Continuous));
    let mut action = controller.step(Event::Start { position: Position::new(500) });
    let mut observations = Vec::new();
    for _ in 0..256 {
        if matches!(action, Action::Monitor { .. }) {
            assert_focused_monitor(action);
            assert_eq!(observations.first(), Some(&Position::new(500)));
            let reversal = observations.windows(2).position(|pair| pair[1] > pair[0]).unwrap();
            assert!(observations[..=reversal].iter().any(|position| position.units() < 400));
            assert!(observations[reversal + 1..].iter().any(|position| position.units() > 620));
            assert!(observations.windows(2).any(|pair| pair[0] == pair[1] && pair[0].units() < 400));
            return;
        }
        if let Action::Observe { position, .. } = action {
            observations.push(position);
        }
        action = synthetic_response(&mut controller, action, SINGLE, LibcameraMode::Continuous);
    }
    panic!("synthetic interior reversal did not focus within the host bound");
}

#[test]
fn synthetic_libcamera_low_confidence_continuous_search_remains_monitorable() {
    for fixture in [FLAT, MULTI] {
        let (mut controller, action) = synthetic_continuous_monitor(fixture, Position::new(0));
        assert!(matches!(action, Action::Monitor { status: FocusStatus::Failed, .. }), "{action:?}");
        assert_eq!(controller.step(Event::Cancel), Action::Fail(Failure::Cancelled));
    }
}
