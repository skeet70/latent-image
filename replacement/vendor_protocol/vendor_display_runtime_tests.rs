use super::*;

fn selection() -> Selection {
    Selection { configuration: ConfigurationType(1), format: OutputFormat(2), primary: PrimarySource(1), bridge: BridgeMode(0), timing: TimingSelector::parse(4).unwrap(), source: SourceSelector(0), hardware: HardwareSelector(0) }
}

#[test]
fn reselecting_type_three_preserves_configuration_type_and_ignores_hardware_input() {
    let mut state = SelectionState::adopt(selection(), PrimarySource(2), BridgeMode(1), true);
    let requested = Selection { configuration: ConfigurationType(3), primary: PrimarySource(0), source: SourceSelector(9), hardware: HardwareSelector(99), ..selection() };
    state.select(requested);
    let observed = state.snapshot();
    assert_eq!(observed.configuration, ConfigurationType(1));
    assert_eq!(observed.source, SourceSelector(0));
    assert_eq!(observed.hardware, HardwareSelector(0));
    assert!(state.old_engine_is_one());
    assert!(!state.new_engine_is_one());
}

#[test]
fn external_selection_preserves_source_and_tracks_engine_transition() {
    let mut state = SelectionState::adopt(selection(), PrimarySource(2), BridgeMode(1), false);
    let requested = Selection { bridge: BridgeMode(1), source: SourceSelector(8), ..selection() };
    assert!(!state.unchanged(requested));
    state.select(requested);
    assert_eq!(state.snapshot(), requested);
    assert!(state.old_engine_is_one());
    assert!(!state.new_engine_is_one());
    assert!(state.unchanged(requested));
}

#[test]
fn internal_selection_ignores_source_for_equality() {
    let state = SelectionState::adopt(selection(), PrimarySource(1), BridgeMode(0), false);
    assert!(state.unchanged(Selection { source: SourceSelector(10), ..selection() }));
    assert!(!state.unchanged(Selection { hardware: HardwareSelector(1), ..selection() }));
}

#[test]
fn actual_panel_transition_target_determines_current_output() {
    use display_scanout_panel::panel_actor::{PanelState, TransitionTarget};
    let expected = [
        OutputMode::Disabled, OutputMode::Disabled, OutputMode::Panel, OutputMode::External,
        OutputMode::Panel, OutputMode::External, OutputMode::Panel, OutputMode::External,
        OutputMode::Panel, OutputMode::External, OutputMode::Disabled, OutputMode::Disabled,
        OutputMode::Panel, OutputMode::External,
    ];
    let mut panel = PanelState::new(None);
    for (target, output) in expected.into_iter().enumerate() {
        panel.target = TransitionTarget::parse(target as u32).unwrap();
        assert_eq!(current_output(&panel), output);
    }
}
