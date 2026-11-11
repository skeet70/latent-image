use crate::recording_capture_backends::*;

struct Camera {
    words: [u32; 0xc3a0 / 4],
    states: [u16; 4],
    message: u16,
    messages: [u16; 4],
    message_count: usize,
    deferred: u32,
    notified: u32,
    output_byte: u8,
    setting_result: i32,
}
impl Default for Camera {
    fn default() -> Self { Self { words: [0; 0xc3a0 / 4], states: [0; 4], message: 0, messages: [0; 4], message_count: 0, deferred: 0, notified: 0, output_byte: 0, setting_result: 1 } }
}
fn index(backend: Backend) -> usize { match backend { Backend::Settings => 0, Backend::Auxiliary => 1, Backend::Motor => 2, Backend::Slot => 3 } }
impl CaptureBackendIo for Camera {
    fn read(&self, offset: RecordingOffset) -> u32 { self.words[offset.0 as usize / 4] }
    fn write(&mut self, offset: RecordingOffset, value: u32) { self.words[offset.0 as usize / 4] = value; }
    fn read_guest(&self, address: GuestAddress) -> u32 { match address.0 { 0x1000 => 0, 0x1054 => 0, 0x2000 => 1 << 0x15, 0x2004 | 0x2008 => 0, _ => panic!("unexpected guest read") } }
    fn write_guest_byte(&mut self, _address: GuestAddress, value: u8) { self.output_byte = value; }
    fn state(&self, backend: Backend) -> u16 { self.states[index(backend)] }
    fn set_state(&mut self, backend: Backend, state: u16) { self.states[index(backend)] = state; }
    fn set_row(&mut self, _backend: Backend, _row: u16) {}
    fn message(&self) -> u16 { self.message }
    fn repeat_current(&mut self) { self.deferred += 1; }
    fn defer(&mut self) { self.deferred += 1; }
    fn missing_state(&mut self) { panic!("unexpected missing state"); }
    fn post_message(&mut self, message: u16) { self.messages[self.message_count] = message; self.message_count += 1; }
    fn notify(&mut self) { self.notified += 1; }
    fn submit_motor(&mut self, _mode: u32) { self.post_message(0x13bc); }
    fn submit_slot(&mut self, _mode: u32) { self.post_message(0x13e4); }
    fn group(&mut self, _operation: Operation, _group: SettingGroup, _flags: u8) -> i32 { self.setting_result }
    fn motor_setting(&mut self, _argument: GuestAddress) -> i32 { self.setting_result }
    fn selected_settings(&mut self, _kind: u32, _settings: GuestAddress, _selected: GuestAddress) -> i32 { self.setting_result }
    fn output_settings(&mut self, _kind: u32, _settings: GuestAddress) -> i32 { self.setting_result }
    fn configuration_678(&self) -> u32 { 0 }
}
#[test]
fn auxiliary_start_requires_its_completion_message() {
    let mut camera = Camera::default();
    assert_eq!(auxiliary_request(&mut camera, 1, 7), 0);
    assert_eq!(&camera.messages[..camera.message_count], &[0x13bb]);
    assert_eq!(auxiliary_request(&mut camera, 1, 8), 0);
    assert_eq!(camera.deferred, 1);
    assert_eq!(camera.message_count, 1);
    camera.message = 0x13bb;
    assert_eq!(auxiliary_request(&mut camera, 3, 0), 1);
    assert_eq!(auxiliary_request(&mut camera, 2, 0), 1);
}
#[test]
fn motor_poll_copies_existing_output_without_replacing_request() {
    let mut camera = Camera::default();
    camera.write(RecordingOffset(0x945c), 0xaabbccdd);
    assert_eq!(motor_request(&mut camera, 1, 3, None), 0);
    camera.message = 0x13bc;
    assert_eq!(motor_request(&mut camera, 2, 99, Some(GuestAddress(0x3000))), 1);
    assert_eq!(camera.output_byte, 0xdd);
    assert_eq!(camera.read(RecordingOffset(0x9450)), 3);
}
#[test]
fn slot_publishes_value_only_when_completion_is_consumed() {
    let mut camera = Camera::default();
    assert_eq!(slot_request(&mut camera, Some([1, 0x1234])), 1);
    assert_eq!(camera.notified, 0);
    camera.message = 0x13e4;
    assert_eq!(slot_request(&mut camera, Some([2, 0])), 0);
    assert_eq!(camera.output_byte, 0x34);
    assert_eq!(camera.notified, 1);
    assert_eq!(slot_request(&mut camera, Some([0, 0])), 0);
}
#[test]
fn setting_failure_preserves_uncompleted_bitmap_for_resume() {
    let mut camera = Camera { setting_result: -7, ..Camera::default() };
    assert_eq!(settings_request(&mut camera, 2, GuestAddress(0x1000), GuestAddress(0x2000), GuestAddress(0)), -7);
    assert_eq!(camera.read(RecordingOffset(0x93e0)), 1 << 0x15);
    camera.setting_result = 1;
    assert_eq!(settings_request(&mut camera, 0x19, GuestAddress(0x1000), GuestAddress(0), GuestAddress(0)), 1);
    assert_eq!(camera.read(RecordingOffset(0x93e0)), 0);
}
#[test]
fn valid_stale_state_indexes_the_flat_transition_table() {
    let mut camera = Camera::default();
    camera.states[index(Backend::Auxiliary)] = 6;
    assert_eq!(auxiliary_request(&mut camera, 2, 0), 1);
    assert_eq!(camera.deferred, 0);
}
#[test]
fn empty_setting_bitmap_does_not_dereference_absent_settings() {
    let mut camera = Camera::default();
    assert_eq!(settings_request(&mut camera, 2, GuestAddress(0), GuestAddress(0), GuestAddress(0)), 1);
}
