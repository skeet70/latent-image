use super::*;

fn inputs() -> InitialSettingInputs {
    InitialSettingInputs { capture_mode: false, selector: 0, capture_enabled: false, category_22c0: false, alternate_output: false, category_2080: false, recording_disabled: false, recording_selector: 0, playback_recording: CommandToken(0) }
}

#[test]
fn initial_record_has_seven_command_words() {
    let record = initial_setting_record(inputs());
    assert_eq!(record, SettingRecord { mode: CommandToken(129), selector: CommandToken(277), flip: CommandToken(284), source: CommandToken(285), output: CommandToken(21), recording: CommandToken(21), enabled: CommandToken(129) });
    assert_eq!(decode_record(record), Ok(DecodeResult::Configuration(ControllerConfiguration { mode: 0, selector: 0, flip: 0, output: 3, recording: 2, enabled: 0, source: 0 })));
}

#[test]
fn capture_output_and_recording_priorities_are_preserved() {
    let mut input = inputs();
    input.capture_mode = true;
    input.capture_enabled = true;
    input.category_22c0 = true;
    input.alternate_output = true;
    input.category_2080 = true;
    input.recording_selector = 2;
    let record = initial_setting_record(input);
    assert_eq!(record.mode, CommandToken(128));
    assert_eq!(record.output, CommandToken(259));
    assert_eq!(record.recording, CommandToken(204));
    input.category_22c0 = false;
    assert_eq!(initial_setting_record(input).output, CommandToken(260));
    input.alternate_output = false;
    assert_eq!(initial_setting_record(input).output, CommandToken(261));
    input.recording_disabled = true;
    assert_eq!(initial_setting_record(input).recording, CommandToken(21));
}

#[test]
fn selector_disable_precedes_validation_of_other_words() {
    let record = SettingRecord { mode: CommandToken(u32::MAX), selector: CommandToken(274), flip: CommandToken(u32::MAX), source: CommandToken(u32::MAX), output: CommandToken(u32::MAX), recording: CommandToken(u32::MAX), enabled: CommandToken(u32::MAX) };
    assert_eq!(decode_record(record), Ok(DecodeResult::Disabled));
}

#[derive(Default)]
struct Sink { configuration: Option<ControllerConfiguration>, source: Option<u32>, configure_status: i32, source_status: i32 }
impl InitialRecordSink for Sink {
    fn configure(&mut self, configuration: ControllerConfiguration) -> i32 { self.configuration = Some(configuration); self.configure_status }
    fn record_source(&mut self, source: u32) -> i32 { self.source = Some(source); self.source_status }
}

#[test]
fn source_is_published_even_after_configuration_failure() {
    let mut sink = Sink { configure_status: -30, ..Sink::default() };
    assert_eq!(publish_record(&mut sink, initial_setting_record(inputs())), -63);
    assert_eq!(sink.source, Some(0));
}

#[test]
fn malformed_record_publishes_nothing() {
    let mut record = initial_setting_record(inputs());
    record.enabled = CommandToken(130);
    let mut sink = Sink::default();
    assert_eq!(publish_record(&mut sink, record), -63);
    assert_eq!(sink.configuration, None);
    assert_eq!(sink.source, None);
}

#[test]
fn all_mode_tokens_reach_their_controller_values() {
    for (token, expected) in [(128, 1), (173, 2), (131, 3), (207, 4), (302, 5), (303, 6), (304, 7), (305, 8), (306, 9), (307, 10), (308, 11), (309, 12), (310, 13), (129, 0), (0, 0)] {
        let mut record = initial_setting_record(inputs());
        record.mode = CommandToken(token);
        let mut sink = Sink::default();
        assert_eq!(publish_record(&mut sink, record), 0);
        assert_eq!(sink.configuration.unwrap().mode, expected);
    }
}

#[test]
fn all_selector_and_recording_tokens_decode() {
    for (selector, expected) in [(275, 2), (276, 1), (277, 0), (278, 3)] {
        for (recording, expected_recording) in [(204, 0), (203, 1), (21, 2)] {
            let mut record = initial_setting_record(inputs());
            record.selector = CommandToken(selector);
            record.recording = CommandToken(recording);
            let mut sink = Sink::default();
            assert_eq!(publish_record(&mut sink, record), 0);
            let configuration = sink.configuration.unwrap();
            assert_eq!(configuration.selector, expected);
            assert_eq!(configuration.recording, expected_recording);
        }
    }
}

#[test]
fn invalid_each_field_rejects_without_effects() {
    for field in 0..6 {
        let mut record = initial_setting_record(inputs());
        match field {
            0 => record.selector = CommandToken(273),
            1 => record.flip = CommandToken(282),
            2 => record.source = CommandToken(288),
            3 => record.output = CommandToken(258),
            4 => record.recording = CommandToken(205),
            _ => record.enabled = CommandToken(127),
        }
        let mut sink = Sink::default();
        assert_eq!(publish_record(&mut sink, record), -63);
        assert_eq!(sink.configuration, None);
        assert_eq!(sink.source, None);
    }
}
