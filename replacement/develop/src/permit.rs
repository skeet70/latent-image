use crate::media_permit::{FrameKey, FrameKind, PermitWord};

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Frame(i32);

impl Frame {
    const ABSENT: Self = Self(-1);
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct CaptureId(u32);

#[repr(C)]
struct PermitPayload {
    permit: PermitWord,
    key: FrameKey,
}

#[repr(C)]
struct Request<'a> {
    command: u16,
    source: u8,
    destination: u8,
    bytes: u32,
    payload: &'a PermitPayload,
}

// The retained mirror stores payload words at +0x0c and +0x10.
#[repr(C)]
struct Mirror {
    command: u16,
    source: u8,
    destination: u8,
    bytes: u32,
    body: u32,
    words: [u32; 64],
}

#[derive(Clone, Copy)]
enum Decode {
    Selected,
    Key,
    CaptureFirst,
    CaptureSecond,
    MultiExposure,
    Subframe,
    ReadySubframe,
    FirstOfKind,
    Sequence,
    Remembered,
    CancelSensitive,
    Unknown,
}

impl Decode {
    fn from_command(command: u16) -> Self {
        match command {
            0x1334..=0x1336 | 0x1339 | 0x133b..=0x1341 | 0x1345..=0x1348
            | 0x134b | 0x1377..=0x137a | 0x137c..=0x137d | 0x137f
            | 0x1388..=0x138c => Self::Selected,
            0x134e | 0x1367 => Self::Key,
            0x1b1b | 0x1b35..=0x1b3f | 0x9b33..=0x9b34 => Self::CaptureFirst,
            0x9b00 => Self::CaptureSecond,
            0x1349 => Self::MultiExposure,
            0x137b => Self::Subframe,
            0x137e => Self::ReadySubframe,
            0x1386 => Self::FirstOfKind,
            0x13df => Self::Sequence,
            0x93e0 => Self::Remembered,
            0x9b08 => Self::CancelSensitive,
            _ => Self::Unknown,
        }
    }
}

trait Frames {
    fn selected(&mut self, kind: FrameKind) -> Frame;
    fn resolve_key(&mut self, key: FrameKey) -> Frame;
    fn resolve_capture(&mut self, capture: CaptureId) -> Frame;
    fn processing_kind(&mut self, frame: Frame) -> Option<FrameKind>;
    fn subframe(&mut self, frame: Frame) -> Frame;
    fn ready(&mut self, frame: Frame) -> bool;
    fn first_of_kind(&mut self) -> Frame;
    fn sequence(&mut self) -> Frame;
    fn remembered(&mut self) -> Frame;
    fn cancelled(&mut self) -> bool;
    fn pending_capture(&mut self) -> CaptureId;
    fn multi_exposure(&mut self, frame: Frame) -> Frame;
    fn needs_remap(&mut self, frame: Frame) -> bool;
    fn remap(&mut self, command: u16, frame: Frame) -> Frame;
}

fn decode(runtime: &mut impl Frames, command: u16, first: u32, second: u32) -> Frame {
    let frame = match Decode::from_command(command) {
        Decode::Selected => runtime.selected(FrameKind::Zero),
        Decode::Key => runtime.resolve_key(FrameKey(second)),
        Decode::CaptureFirst => runtime.resolve_capture(CaptureId(first)),
        Decode::CaptureSecond => runtime.resolve_capture(CaptureId(second)),
        Decode::MultiExposure => {
            let selected = runtime.selected(FrameKind::Zero);
            if first == 1 { runtime.multi_exposure(selected) } else { selected }
        }
        Decode::Subframe | Decode::ReadySubframe => {
            let selected = runtime.selected(FrameKind::Zero);
            if runtime.processing_kind(selected) == Some(FrameKind::Two) {
                let subframe = runtime.subframe(selected);
                if matches!(Decode::from_command(command), Decode::Subframe) {
                    subframe
                } else if runtime.ready(subframe) {
                    runtime.subframe(selected)
                } else {
                    selected
                }
            } else {
                selected
            }
        }
        Decode::FirstOfKind => runtime.first_of_kind(),
        Decode::Sequence => runtime.sequence(),
        Decode::Remembered => runtime.remembered(),
        Decode::CancelSensitive => {
            if runtime.cancelled() { return Frame::ABSENT; }
            let capture = runtime.pending_capture();
            runtime.resolve_capture(capture)
        }
        Decode::Unknown => Frame::ABSENT,
    };
    if frame != Frame::ABSENT && runtime.needs_remap(frame) {
        runtime.remap(command, frame)
    } else {
        frame
    }
}

unsafe extern "C" {
    fn xm1_dp_trace_task() -> u32;
    fn xm1_dp_trace_text(task: u32, text: *const u8);
    fn xm1_dp_receive(request: *const Request<'_>);
    fn xm1_dp_dispatch();
    fn xm1_dp_mirror(source: u32, mirror: *mut Mirror);
    fn xm1_dp_selected(kind: u32) -> i32;
    fn xm1_dp_capture_frame(capture: u32) -> i32;
    fn xm1_dp_processing_kind(frame: i32) -> u32;
    fn xm1_dp_subframe(frame: i32) -> i32;
    fn xm1_dp_ready(bitmap: *const u8, bit: u32) -> u32;
    fn xm1_dp_first_of_kind(kind: u32) -> i32;
    fn xm1_dp_last_capture(frame: i32) -> u32;
    fn xm1_dp_sequence_frame(capture: u32, sequence: u32) -> i32;
    fn xm1_dp_multi_exposure(frame: i32) -> i32;
    fn xm1_dp_kind_group(frame: i32) -> u32;
    fn xm1_dp_remap(command: u32, frame: i32) -> i32;
}

const CONTROL: usize = 0x0157_bdf0;

fn word(address: usize) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

fn byte(address: usize) -> u8 {
    unsafe { core::ptr::read_volatile(address as *const u8) }
}

struct StockFrames;

impl Frames for StockFrames {
    fn selected(&mut self, kind: FrameKind) -> Frame {
        Frame(unsafe { xm1_dp_selected(kind.word()) })
    }
    fn resolve_key(&mut self, key: FrameKey) -> Frame {
        Frame(crate::media_permit_stock::resolve_key(key.0))
    }
    fn resolve_capture(&mut self, capture: CaptureId) -> Frame {
        Frame(unsafe { xm1_dp_capture_frame(capture.0) })
    }
    fn processing_kind(&mut self, frame: Frame) -> Option<FrameKind> {
        FrameKind::from_word(unsafe { xm1_dp_processing_kind(frame.0) })
    }
    fn subframe(&mut self, frame: Frame) -> Frame {
        Frame(unsafe { xm1_dp_subframe(frame.0) })
    }
    fn ready(&mut self, frame: Frame) -> bool {
        let address = (CONTROL as u32).wrapping_add(0x1ab28)
            .wrapping_add((frame.0 as u32).wrapping_mul(0x1d0));
        unsafe { xm1_dp_ready(address as *const u8, 2) == 1 }
    }
    fn first_of_kind(&mut self) -> Frame {
        Frame(unsafe { xm1_dp_first_of_kind(1) })
    }
    fn sequence(&mut self) -> Frame {
        let selected = self.selected(FrameKind::One);
        let capture = unsafe { xm1_dp_last_capture(selected.0) };
        Frame(unsafe { xm1_dp_sequence_frame(capture, byte(CONTROL + 0x81c8) as u32) })
    }
    fn remembered(&mut self) -> Frame { Frame(word(CONTROL + 0x1a9cc) as i32) }
    fn cancelled(&mut self) -> bool { byte(CONTROL + 0x66e) == 1 }
    fn pending_capture(&mut self) -> CaptureId { CaptureId(word(CONTROL + 0xdfc)) }
    fn multi_exposure(&mut self, frame: Frame) -> Frame {
        Frame(unsafe { xm1_dp_multi_exposure(frame.0) })
    }
    fn needs_remap(&mut self, frame: Frame) -> bool {
        unsafe { xm1_dp_kind_group(frame.0) == 1 }
    }
    fn remap(&mut self, command: u16, frame: Frame) -> Frame {
        Frame(unsafe { xm1_dp_remap(command as u32, frame.0) })
    }
}

#[no_mangle]
pub extern "C" fn xm1_develop_permit_request(permit: u32, key: u32) {
    let payload = PermitPayload { permit: PermitWord(permit), key: FrameKey(key) };
    unsafe {
        xm1_dp_trace_text(xm1_dp_trace_task() & 0xff, 0x00e6_7e1c as *const u8);
        let request = Request {
            command: 0x1367,
            source: xm1_dp_trace_task() as u8,
            destination: xm1_dp_trace_task() as u8,
            bytes: 8,
            payload: &payload,
        };
        xm1_dp_receive(&request);
        xm1_dp_dispatch();
    }
}

#[no_mangle]
pub extern "C" fn xm1_develop_decode() -> i32 {
    let mut mirror = core::mem::MaybeUninit::<Mirror>::uninit();
    unsafe {
        xm1_dp_mirror(0, mirror.as_mut_ptr());
        let pointer = mirror.as_ptr();
        let command = core::ptr::addr_of!((*pointer).command).read();
        let words = core::ptr::addr_of!((*pointer).words).cast::<u32>();
        decode(&mut StockFrames, command, words.read(), words.add(1).read()).0
    }
}

#[no_mangle]
pub extern "C" fn xm1_develop_permit_primary() -> i32 {
    crate::media_permit_stock::apply_mirror();
    0
}

#[no_mangle]
pub extern "C" fn xm1_develop_permit_secondary() -> i32 {
    crate::media_permit_stock::apply_mirror();
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scenario {
        processing_kind: Option<FrameKind>,
        ready: bool,
        cancelled: bool,
        group: u32,
        key_result: Frame,
    }

    impl Scenario {
        fn normal() -> Self {
            Self {
                processing_kind: Some(FrameKind::Two),
                ready: true,
                cancelled: false,
                group: 0,
                key_result: Frame(30),
            }
        }
    }

    impl Frames for Scenario {
        fn selected(&mut self, kind: FrameKind) -> Frame { Frame(10 + kind.word() as i32) }
        fn resolve_key(&mut self, key: FrameKey) -> Frame {
            if key == FrameKey(22) { self.key_result } else { Frame::ABSENT }
        }
        fn resolve_capture(&mut self, capture: CaptureId) -> Frame { Frame(capture.0 as i32) }
        fn processing_kind(&mut self, _: Frame) -> Option<FrameKind> { self.processing_kind }
        fn subframe(&mut self, _: Frame) -> Frame { Frame(20) }
        fn ready(&mut self, _: Frame) -> bool { self.ready }
        fn first_of_kind(&mut self) -> Frame { Frame(40) }
        fn sequence(&mut self) -> Frame { Frame(50) }
        fn remembered(&mut self) -> Frame { Frame(60) }
        fn cancelled(&mut self) -> bool { self.cancelled }
        fn pending_capture(&mut self) -> CaptureId { CaptureId(70) }
        fn multi_exposure(&mut self, frame: Frame) -> Frame { Frame(frame.0 + 1) }
        fn needs_remap(&mut self, _: Frame) -> bool { self.group == 1 }
        fn remap(&mut self, _: u16, frame: Frame) -> Frame { Frame(frame.0 + 100) }
    }

    #[test]
    fn complete_message_routes() {
        let selected = [
            0x1334, 0x1335, 0x1336, 0x1339, 0x133b, 0x133c, 0x133d,
            0x133e, 0x133f, 0x1340, 0x1341, 0x1345, 0x1346, 0x1347,
            0x1348, 0x134b, 0x1377, 0x1378, 0x1379, 0x137a, 0x137c,
            0x137d, 0x137f, 0x1388, 0x1389, 0x138a, 0x138b, 0x138c,
        ];
        let capture = [
            0x1b1b, 0x1b35, 0x1b36, 0x1b37, 0x1b38, 0x1b39, 0x1b3a,
            0x1b3b, 0x1b3c, 0x1b3d, 0x1b3e, 0x1b3f, 0x9b33, 0x9b34,
        ];
        for command in selected {
            assert_eq!(decode(&mut Scenario::normal(), command, 11, 22), Frame(10));
        }
        for command in capture {
            assert_eq!(decode(&mut Scenario::normal(), command, 11, 22), Frame(11));
        }
        for (command, expected) in [
            (0x1349, 10), (0x134e, 30), (0x1367, 30), (0x137b, 20),
            (0x137e, 20), (0x1386, 40), (0x13df, 50), (0x93e0, 60),
            (0x9b00, 22), (0x9b08, 70),
        ] {
            assert_eq!(decode(&mut Scenario::normal(), command, 11, 22), Frame(expected));
        }
    }

    #[test]
    fn incorrect_controls_are_not_selected_messages() {
        for command in [
            0, 0x1333, 0x1337, 0x1338, 0x133a, 0x1342, 0x1344,
            0x134a, 0x134c, 0x134d, 0x134f, 0x1366, 0x1368,
            0x1376, 0x1380, 0x1385, 0x1387, 0x138d, 0x13de,
            0x13e0, 0x1b1a, 0x1b1c, 0x1b34, 0x1b40, 0x93df,
            0x93e1, 0x9aff, 0x9b01, 0x9b07, 0x9b09, 0x9b32,
            0x9b35, 0xffff,
        ] {
            assert_eq!(decode(&mut Scenario::normal(), command, 11, 22), Frame::ABSENT);
        }
    }

    #[test]
    fn cancellation_and_unresolved_keys_remain_absent() {
        let mut scenario = Scenario::normal();
        scenario.cancelled = true;
        scenario.group = 1;
        assert_eq!(decode(&mut scenario, 0x9b08, 11, 22), Frame::ABSENT);
        scenario.key_result = Frame::ABSENT;
        assert_eq!(decode(&mut scenario, 0x1367, 11, 22), Frame::ABSENT);
        assert_eq!(decode(&mut scenario, 0x134e, 11, 22), Frame::ABSENT);
        assert_eq!(decode(&mut scenario, 0x1367, 11, 23), Frame::ABSENT);
    }

    #[test]
    fn conditional_frame_selection_and_group_remapping() {
        let mut scenario = Scenario::normal();
        scenario.ready = false;
        assert_eq!(decode(&mut scenario, 0x137e, 11, 22), Frame(10));
        assert_eq!(decode(&mut scenario, 0x137b, 11, 22), Frame(20));
        scenario.processing_kind = Some(FrameKind::One);
        assert_eq!(decode(&mut scenario, 0x137b, 11, 22), Frame(10));
        assert_eq!(decode(&mut scenario, 0x1349, 1, 22), Frame(11));
        assert_eq!(decode(&mut scenario, 0x1349, 2, 22), Frame(10));
        scenario.group = 1;
        assert_eq!(decode(&mut scenario, 0x1367, 11, 22), Frame(130));
        assert_eq!(decode(&mut scenario, 0x9b00, 11, 22), Frame(122));
        scenario.group = 2;
        assert_eq!(decode(&mut scenario, 0x1367, 11, 22), Frame(30));
    }
}
