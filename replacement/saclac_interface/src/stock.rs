use crate::*;

pub mod area_measurement;
pub mod calculation_result;
pub mod capture_job;
pub mod capture_results;
pub mod exposure;
pub mod pipeline;
pub mod search;
pub mod shared;
pub mod statistics;

pub const fn record_handle_from_a2(word: u32) -> RecordHandle { RecordHandle(word) }

pub const fn record_handle_word(handle: RecordHandle) -> u32 { handle.0 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandDescriptor {
    pub kind: CommandKind,
    pub code: u8,
    pub producer: u32,
    pub payload_bytes: usize,
}

pub const COMMANDS: [CommandDescriptor; 40] = [
    CommandDescriptor { kind: CommandKind::Start, code: 0, producer: 0xef8db4, payload_bytes: 168 },
    CommandDescriptor { kind: CommandKind::Stop, code: 1, producer: 0xef8e54, payload_bytes: 4 },
    CommandDescriptor { kind: CommandKind::Command02, code: 2, producer: 0xef8e7c, payload_bytes: 28 },
    CommandDescriptor { kind: CommandKind::Command03, code: 3, producer: 0xef8ed0, payload_bytes: 24 },
    CommandDescriptor { kind: CommandKind::LensBringUp, code: 4, producer: 0xef8f14, payload_bytes: 24 },
    CommandDescriptor { kind: CommandKind::LensStop, code: 5, producer: 0xef8f58, payload_bytes: 12 },
    CommandDescriptor { kind: CommandKind::HalfPress, code: 6, producer: 0xef8fa0, payload_bytes: 64 },
    CommandDescriptor { kind: CommandKind::Command07, code: 7, producer: 0xef9064, payload_bytes: 60 },
    CommandDescriptor { kind: CommandKind::Command08, code: 8, producer: 0xef913c, payload_bytes: 40 },
    CommandDescriptor { kind: CommandKind::Command09, code: 9, producer: 0xef91a0, payload_bytes: 16 },
    CommandDescriptor { kind: CommandKind::Command0a, code: 10, producer: 0xef91e0, payload_bytes: 8 },
    CommandDescriptor { kind: CommandKind::HalfRelease, code: 11, producer: 0xef920c, payload_bytes: 4 },
    CommandDescriptor { kind: CommandKind::Command0c, code: 12, producer: 0xef9234, payload_bytes: 12 },
    CommandDescriptor { kind: CommandKind::Autofocus, code: 13, producer: 0xef9264, payload_bytes: 160 },
    CommandDescriptor { kind: CommandKind::LensDrive, code: 14, producer: 0xef9320, payload_bytes: 20 },
    CommandDescriptor { kind: CommandKind::Cancel, code: 15, producer: 0xef9370, payload_bytes: 8 },
    CommandDescriptor { kind: CommandKind::Command10, code: 16, producer: 0xef939c, payload_bytes: 8 },
    CommandDescriptor { kind: CommandKind::Command11, code: 17, producer: 0xef93c4, payload_bytes: 20 },
    CommandDescriptor { kind: CommandKind::Command13, code: 19, producer: 0xef9404, payload_bytes: 4 },
    CommandDescriptor { kind: CommandKind::Command14, code: 20, producer: 0xef942c, payload_bytes: 20 },
    CommandDescriptor { kind: CommandKind::Command15, code: 21, producer: 0xef9468, payload_bytes: 8 },
    CommandDescriptor { kind: CommandKind::Command16, code: 22, producer: 0xef9494, payload_bytes: 4 },
    CommandDescriptor { kind: CommandKind::Command17, code: 23, producer: 0xef9510, payload_bytes: 40 },
    CommandDescriptor { kind: CommandKind::Prepare, code: 26, producer: 0xef955c, payload_bytes: 20 },
    CommandDescriptor { kind: CommandKind::Command1d, code: 29, producer: 0xef9598, payload_bytes: 12 },
    CommandDescriptor { kind: CommandKind::Command1e, code: 30, producer: 0xef95c8, payload_bytes: 8 },
    CommandDescriptor { kind: CommandKind::Command1f, code: 31, producer: 0xef95f8, payload_bytes: 20 },
    CommandDescriptor { kind: CommandKind::Command20, code: 32, producer: 0xef9650, payload_bytes: 8 },
    CommandDescriptor { kind: CommandKind::Command21, code: 33, producer: 0xef967c, payload_bytes: 8 },
    CommandDescriptor { kind: CommandKind::Command22, code: 34, producer: 0xef96a8, payload_bytes: 8 },
    CommandDescriptor { kind: CommandKind::Command23, code: 35, producer: 0xef96d4, payload_bytes: 12 },
    CommandDescriptor { kind: CommandKind::Command24, code: 36, producer: 0xef9718, payload_bytes: 20 },
    CommandDescriptor { kind: CommandKind::Command25, code: 37, producer: 0xef9804, payload_bytes: 12 },
    CommandDescriptor { kind: CommandKind::Prepare26, code: 38, producer: 0xef9844, payload_bytes: 8 },
    CommandDescriptor { kind: CommandKind::Command27, code: 39, producer: 0xef9870, payload_bytes: 8 },
    CommandDescriptor { kind: CommandKind::Command28, code: 40, producer: 0xef98bc, payload_bytes: 4 },
    CommandDescriptor { kind: CommandKind::Command29, code: 41, producer: 0xef98e4, payload_bytes: 8 },
    CommandDescriptor { kind: CommandKind::Command2b, code: 43, producer: 0xef9988, payload_bytes: 16 },
    CommandDescriptor { kind: CommandKind::Command2c, code: 44, producer: 0xef94bc, payload_bytes: 8 },
    CommandDescriptor { kind: CommandKind::Command2d, code: 45, producer: 0xef94e8, payload_bytes: 4 },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParameterBlock { pub address: u32, pub capacity: usize }

pub const PARAMETER_BLOCKS: [ParameterBlock; 46] = [
    ParameterBlock { address: 0x169cf58, capacity: 168 },
    ParameterBlock { address: 0x169d000, capacity: 4 },
    ParameterBlock { address: 0x169d004, capacity: 28 },
    ParameterBlock { address: 0x169d020, capacity: 24 },
    ParameterBlock { address: 0x169d038, capacity: 24 },
    ParameterBlock { address: 0x169d050, capacity: 12 },
    ParameterBlock { address: 0x169d05c, capacity: 64 },
    ParameterBlock { address: 0x169d09c, capacity: 60 },
    ParameterBlock { address: 0x169d0d8, capacity: 40 },
    ParameterBlock { address: 0x169d100, capacity: 16 },
    ParameterBlock { address: 0x169d110, capacity: 8 },
    ParameterBlock { address: 0x169d118, capacity: 4 },
    ParameterBlock { address: 0x169d11c, capacity: 12 },
    ParameterBlock { address: 0x169d128, capacity: 160 },
    ParameterBlock { address: 0x169d1c8, capacity: 20 },
    ParameterBlock { address: 0x169d1dc, capacity: 8 },
    ParameterBlock { address: 0x169d1e4, capacity: 8 },
    ParameterBlock { address: 0x169d1ec, capacity: 20 },
    ParameterBlock { address: 0x169d200, capacity: 8 },
    ParameterBlock { address: 0x169d208, capacity: 4 },
    ParameterBlock { address: 0x169d20c, capacity: 20 },
    ParameterBlock { address: 0x169d220, capacity: 8 },
    ParameterBlock { address: 0x169d228, capacity: 4 },
    ParameterBlock { address: 0x169d22c, capacity: 40 },
    ParameterBlock { address: 0x169d254, capacity: 32 },
    ParameterBlock { address: 0x169d274, capacity: 4 },
    ParameterBlock { address: 0x169d278, capacity: 20 },
    ParameterBlock { address: 0x169d28c, capacity: 8 },
    ParameterBlock { address: 0x169d294, capacity: 8 },
    ParameterBlock { address: 0x169d29c, capacity: 12 },
    ParameterBlock { address: 0x169d2a8, capacity: 8 },
    ParameterBlock { address: 0x169d2b0, capacity: 20 },
    ParameterBlock { address: 0x169d2c4, capacity: 8 },
    ParameterBlock { address: 0x169d2cc, capacity: 8 },
    ParameterBlock { address: 0x169d2d4, capacity: 8 },
    ParameterBlock { address: 0x169d2dc, capacity: 12 },
    ParameterBlock { address: 0x169d2e8, capacity: 20 },
    ParameterBlock { address: 0x169d2fc, capacity: 12 },
    ParameterBlock { address: 0x169d308, capacity: 8 },
    ParameterBlock { address: 0x169d310, capacity: 8 },
    ParameterBlock { address: 0x169d318, capacity: 4 },
    ParameterBlock { address: 0x169d31c, capacity: 8 },
    ParameterBlock { address: 0x169d324, capacity: 44 },
    ParameterBlock { address: 0x169d350, capacity: 16 },
    ParameterBlock { address: 0x169d360, capacity: 8 },
    ParameterBlock { address: 0x169d368, capacity: 8 },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackDescriptor {
    pub word: u32,
    pub posts: &'static [u8],
    pub payload_bytes: Option<usize>,
}

pub const CALLBACKS: [CallbackDescriptor; 106] = [
    CallbackDescriptor { word: 0xf14ae4, posts: &[0x3d], payload_bytes: Some(12) },
    CallbackDescriptor { word: 0xf14b14, posts: &[0x39], payload_bytes: Some(12) },
    CallbackDescriptor { word: 0xf14b50, posts: &[0x38], payload_bytes: Some(12) },
    CallbackDescriptor { word: 0xf14b8c, posts: &[0x36], payload_bytes: Some(12) },
    CallbackDescriptor { word: 0xf14bc8, posts: &[0x35], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf14be4, posts: &[0x33, 0x34], payload_bytes: Some(16) },
    CallbackDescriptor { word: 0xf15070, posts: &[0x3c], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf1509c, posts: &[0x40], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf150c4, posts: &[0x30], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf150f0, posts: &[0x31, 0x32, 0x2f], payload_bytes: Some(16) },
    CallbackDescriptor { word: 0xf17c14, posts: &[0xa1], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf17c30, posts: &[0x3a], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf17c4c, posts: &[0x35], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf1d154, posts: &[0x90], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf1eddc, posts: &[0x4d, 0x4c], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf1ee10, posts: &[0x8e], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf1fc08, posts: &[0x8f], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf20380, posts: &[0x91], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf203b4, posts: &[0x93], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf20418, posts: &[0x92], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf204fc, posts: &[0x6b], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf20518, posts: &[0x7a], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf20534, posts: &[0x53], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf20550, posts: &[0x52], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf2056c, posts: &[0x51], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf20594, posts: &[0x50], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf205c0, posts: &[0x4e], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf205e8, posts: &[0x4f], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf226e0, posts: &[0x75], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf22708, posts: &[0x80], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf22730, posts: &[0x7f], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2276c, posts: &[0x7e], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf22794, posts: &[0x59], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf227c0, posts: &[0x57], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf227dc, posts: &[0x7c], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf227f8, posts: &[0x7b], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf22928, posts: &[0x58], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf24170, posts: &[0x37], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf2418c, posts: &[0x35], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf241a8, posts: &[0x9b], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf241d4, posts: &[0x9c], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf25728, posts: &[0x5a], payload_bytes: Some(12) },
    CallbackDescriptor { word: 0xf25758, posts: &[0x9d], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf28e94, posts: &[0x33, 0x34], payload_bytes: Some(16) },
    CallbackDescriptor { word: 0xf2903c, posts: &[0x85], payload_bytes: Some(24) },
    CallbackDescriptor { word: 0xf2907c, posts: &[0x83], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2ac70, posts: &[0x5d], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2ac98, posts: &[0x95], payload_bytes: None },
    CallbackDescriptor { word: 0xf2acd0, posts: &[0x94], payload_bytes: None },
    CallbackDescriptor { word: 0xf2ad08, posts: &[0x5c], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf2ad34, posts: &[0x5b], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2c940, posts: &[0x35], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf2c968, posts: &[0x3a], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf2c990, posts: &[0x62], payload_bytes: Some(20) },
    CallbackDescriptor { word: 0xf2c9c4, posts: &[0x5e], payload_bytes: Some(16) },
    CallbackDescriptor { word: 0xf2ca04, posts: &[0x64], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf2ca34, posts: &[0x61], payload_bytes: Some(36) },
    CallbackDescriptor { word: 0xf2ca78, posts: &[0x60, 0x5f, 0x60], payload_bytes: Some(92) },
    CallbackDescriptor { word: 0xf2cad8, posts: &[0x7d], payload_bytes: Some(24) },
    CallbackDescriptor { word: 0xf2e67c, posts: &[0x9a], payload_bytes: None },
    CallbackDescriptor { word: 0xf2e6b4, posts: &[0x99], payload_bytes: Some(12) },
    CallbackDescriptor { word: 0xf2e6e4, posts: &[0x98], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2e710, posts: &[0x97], payload_bytes: Some(48) },
    CallbackDescriptor { word: 0xf2ef0c, posts: &[0x35], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf2ef34, posts: &[0x3a], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf2ef5c, posts: &[0x90], payload_bytes: Some(0) },
    CallbackDescriptor { word: 0xf2f65c, posts: &[0x65], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2f684, posts: &[0x63], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2f6ac, posts: &[0x4b], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf2f6d8, posts: &[0x4a], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2f700, posts: &[0x44], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2f728, posts: &[0x43], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2f750, posts: &[0x42], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2f778, posts: &[0x41], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2f7a0, posts: &[0x40], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf2f7c8, posts: &[0x3f], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf32508, posts: &[0x45], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf33520, posts: &[0x88], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf33568, posts: &[0x87], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf37854, posts: &[0x8a], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf38668, posts: &[0x9a], payload_bytes: Some(6) },
    CallbackDescriptor { word: 0xf39b10, posts: &[0x8b], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf39b3c, posts: &[0x66], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf39b64, posts: &[0x47], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf3a3f8, posts: &[0x8d], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf3a768, posts: &[0x82], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf3a794, posts: &[0x7f], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a7bc, posts: &[0x7e], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a7e4, posts: &[0x80], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a80c, posts: &[0x73], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a834, posts: &[0x71], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a85c, posts: &[0x72], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a884, posts: &[0x75], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a8ac, posts: &[0x77], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a8d4, posts: &[0x70], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a8fc, posts: &[0x6f], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a924, posts: &[0x76], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf3a950, posts: &[0x6e], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a978, posts: &[0x6d], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a9a0, posts: &[0x78], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a9c8, posts: &[0x6c], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3a9f0, posts: &[0x6b], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3aa18, posts: &[0x69], payload_bytes: Some(4) },
    CallbackDescriptor { word: 0xf3aa40, posts: &[0x79], payload_bytes: Some(64) },
    CallbackDescriptor { word: 0xf3c104, posts: &[0x89], payload_bytes: Some(8) },
    CallbackDescriptor { word: 0xf3c14c, posts: &[0x46], payload_bytes: Some(12) },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError { UnknownOperation, Oversize, MissingCallback }

pub fn command(code: u8) -> Option<CommandKind> {
    COMMANDS.iter().find(|entry| entry.code == code).map(|entry| entry.kind)
}

pub fn command_code(kind: CommandKind) -> u8 {
    match kind {
        CommandKind::Start => 0,
        CommandKind::Stop => 1,
        CommandKind::Command02 => 2,
        CommandKind::Command03 => 3,
        CommandKind::LensBringUp => 4,
        CommandKind::LensStop => 5,
        CommandKind::HalfPress => 6,
        CommandKind::Command07 => 7,
        CommandKind::Command08 => 8,
        CommandKind::Command09 => 9,
        CommandKind::Command0a => 10,
        CommandKind::HalfRelease => 11,
        CommandKind::Command0c => 12,
        CommandKind::Autofocus => 13,
        CommandKind::LensDrive => 14,
        CommandKind::Cancel => 15,
        CommandKind::Command10 => 16,
        CommandKind::Command11 => 17,
        CommandKind::Command13 => 19,
        CommandKind::Command14 => 20,
        CommandKind::Command15 => 21,
        CommandKind::Command16 => 22,
        CommandKind::Command17 => 23,
        CommandKind::Prepare => 26,
        CommandKind::Command1d => 29,
        CommandKind::Command1e => 30,
        CommandKind::Command1f => 31,
        CommandKind::Command20 => 32,
        CommandKind::Command21 => 33,
        CommandKind::Command22 => 34,
        CommandKind::Command23 => 35,
        CommandKind::Command24 => 36,
        CommandKind::Command25 => 37,
        CommandKind::Prepare26 => 38,
        CommandKind::Command27 => 39,
        CommandKind::Command28 => 40,
        CommandKind::Command29 => 41,
        CommandKind::Command2b => 43,
        CommandKind::Command2c => 44,
        CommandKind::Command2d => 45,
    }
}

pub fn completion(code: u8) -> Option<Completion> {
    match code {
        0x2f => Some(Completion::Lens2f),
        0x30 => Some(Completion::Lens30),
        0x31 => Some(Completion::Lens31),
        0x32 => Some(Completion::Lens32),
        0x33 => Some(Completion::Lens33),
        0x34 => Some(Completion::Lens34),
        0x35 => Some(Completion::Lens35),
        0x36 => Some(Completion::Lens36),
        0x37 => Some(Completion::Lens37),
        0x38 => Some(Completion::Lens38),
        0x39 => Some(Completion::Lens39),
        0x3a => Some(Completion::Lens3a),
        0x3c => Some(Completion::Lens3c),
        0x3d => Some(Completion::Lens3d),
        0x3f => Some(Completion::Image3f),
        0x40 => Some(Completion::Image40),
        0x41 => Some(Completion::Image41),
        0x42 => Some(Completion::Image42),
        0x43 => Some(Completion::Image43),
        0x44 => Some(Completion::Image44),
        0x45 => Some(Completion::Image45),
        0x46 => Some(Completion::Image46),
        0x47 => Some(Completion::Image47),
        0x4a => Some(Completion::Accessor4a),
        0x4b => Some(Completion::Accessor4b),
        0x4c => Some(Completion::Image4c),
        0x4d => Some(Completion::Image4d),
        0x4e => Some(Completion::Image4e),
        0x4f => Some(Completion::Image4f),
        0x50 => Some(Completion::Image50),
        0x51 => Some(Completion::Image51),
        0x52 => Some(Completion::Image52),
        0x53 => Some(Completion::Image53),
        0x57 => Some(Completion::Image57),
        0x58 => Some(Completion::Image58),
        0x59 => Some(Completion::Image59),
        0x5a => Some(Completion::Image5a),
        0x5b => Some(Completion::Image5b),
        0x5c => Some(Completion::Image5c),
        0x5d => Some(Completion::Image5d),
        0x5e => Some(Completion::Image5e),
        0x5f => Some(Completion::Image5f),
        0x60 => Some(Completion::Image60),
        0x61 => Some(Completion::Image61),
        0x62 => Some(Completion::Image62),
        0x63 => Some(Completion::Image63),
        0x64 => Some(Completion::Image64),
        0x65 => Some(Completion::Image65),
        0x66 => Some(Completion::Image66),
        0x69 => Some(Completion::Statistics69),
        0x6b => Some(Completion::Statistics6b),
        0x6c => Some(Completion::Statistics6c),
        0x6d => Some(Completion::Statistics6d),
        0x6e => Some(Completion::Statistics6e),
        0x6f => Some(Completion::Statistics6f),
        0x70 => Some(Completion::Statistics70),
        0x71 => Some(Completion::Statistics71),
        0x72 => Some(Completion::Statistics72),
        0x73 => Some(Completion::Statistics73),
        0x75 => Some(Completion::Statistics75),
        0x76 => Some(Completion::Statistics76),
        0x77 => Some(Completion::Statistics77),
        0x78 => Some(Completion::Statistics78),
        0x79 => Some(Completion::Statistics79),
        0x7a => Some(Completion::Statistics7a),
        0x7b => Some(Completion::Statistics7b),
        0x7c => Some(Completion::Statistics7c),
        0x7d => Some(Completion::Statistics7d),
        0x7e => Some(Completion::Statistics7e),
        0x7f => Some(Completion::Statistics7f),
        0x80 => Some(Completion::Statistics80),
        0x82 => Some(Completion::Statistics82),
        0x83 => Some(Completion::Statistics83),
        0x85 => Some(Completion::Statistics85),
        0x87 => Some(Completion::Calculation87),
        0x88 => Some(Completion::Calculation88),
        0x89 => Some(Completion::Calculation89),
        0x8a => Some(Completion::Calculation8a),
        0x8b => Some(Completion::Calculation8b),
        0x8d => Some(Completion::Calculation8d),
        0x8e => Some(Completion::Calculation8e),
        0x8f => Some(Completion::Calculation8f),
        0x90 => Some(Completion::Calculation90),
        0x91 => Some(Completion::Calculation91),
        0x92 => Some(Completion::Calculation92),
        0x93 => Some(Completion::Calculation93),
        0x94 => Some(Completion::Calculation94),
        0x95 => Some(Completion::Calculation95),
        0x97 => Some(Completion::Calculation97),
        0x98 => Some(Completion::Calculation98),
        0x99 => Some(Completion::Calculation99),
        0x9a => Some(Completion::Calculation9a),
        0x9b => Some(Completion::Calculation9b),
        0x9c => Some(Completion::Calculation9c),
        0x9d => Some(Completion::Calculation9d),
        0xa1 => Some(Completion::Filea1),
        _ => None,
    }
}

pub fn completion_code(completion: Completion) -> u8 {
    match completion {
        Completion::Lens2f => 0x2f,
        Completion::Lens30 => 0x30,
        Completion::Lens31 => 0x31,
        Completion::Lens32 => 0x32,
        Completion::Lens33 => 0x33,
        Completion::Lens34 => 0x34,
        Completion::Lens35 => 0x35,
        Completion::Lens36 => 0x36,
        Completion::Lens37 => 0x37,
        Completion::Lens38 => 0x38,
        Completion::Lens39 => 0x39,
        Completion::Lens3a => 0x3a,
        Completion::Lens3c => 0x3c,
        Completion::Lens3d => 0x3d,
        Completion::Image3f => 0x3f,
        Completion::Image40 => 0x40,
        Completion::Image41 => 0x41,
        Completion::Image42 => 0x42,
        Completion::Image43 => 0x43,
        Completion::Image44 => 0x44,
        Completion::Image45 => 0x45,
        Completion::Image46 => 0x46,
        Completion::Image47 => 0x47,
        Completion::Accessor4a => 0x4a,
        Completion::Accessor4b => 0x4b,
        Completion::Image4c => 0x4c,
        Completion::Image4d => 0x4d,
        Completion::Image4e => 0x4e,
        Completion::Image4f => 0x4f,
        Completion::Image50 => 0x50,
        Completion::Image51 => 0x51,
        Completion::Image52 => 0x52,
        Completion::Image53 => 0x53,
        Completion::Image57 => 0x57,
        Completion::Image58 => 0x58,
        Completion::Image59 => 0x59,
        Completion::Image5a => 0x5a,
        Completion::Image5b => 0x5b,
        Completion::Image5c => 0x5c,
        Completion::Image5d => 0x5d,
        Completion::Image5e => 0x5e,
        Completion::Image5f => 0x5f,
        Completion::Image60 => 0x60,
        Completion::Image61 => 0x61,
        Completion::Image62 => 0x62,
        Completion::Image63 => 0x63,
        Completion::Image64 => 0x64,
        Completion::Image65 => 0x65,
        Completion::Image66 => 0x66,
        Completion::Statistics69 => 0x69,
        Completion::Statistics6b => 0x6b,
        Completion::Statistics6c => 0x6c,
        Completion::Statistics6d => 0x6d,
        Completion::Statistics6e => 0x6e,
        Completion::Statistics6f => 0x6f,
        Completion::Statistics70 => 0x70,
        Completion::Statistics71 => 0x71,
        Completion::Statistics72 => 0x72,
        Completion::Statistics73 => 0x73,
        Completion::Statistics75 => 0x75,
        Completion::Statistics76 => 0x76,
        Completion::Statistics77 => 0x77,
        Completion::Statistics78 => 0x78,
        Completion::Statistics79 => 0x79,
        Completion::Statistics7a => 0x7a,
        Completion::Statistics7b => 0x7b,
        Completion::Statistics7c => 0x7c,
        Completion::Statistics7d => 0x7d,
        Completion::Statistics7e => 0x7e,
        Completion::Statistics7f => 0x7f,
        Completion::Statistics80 => 0x80,
        Completion::Statistics82 => 0x82,
        Completion::Statistics83 => 0x83,
        Completion::Statistics85 => 0x85,
        Completion::Calculation87 => 0x87,
        Completion::Calculation88 => 0x88,
        Completion::Calculation89 => 0x89,
        Completion::Calculation8a => 0x8a,
        Completion::Calculation8b => 0x8b,
        Completion::Calculation8d => 0x8d,
        Completion::Calculation8e => 0x8e,
        Completion::Calculation8f => 0x8f,
        Completion::Calculation90 => 0x90,
        Completion::Calculation91 => 0x91,
        Completion::Calculation92 => 0x92,
        Completion::Calculation93 => 0x93,
        Completion::Calculation94 => 0x94,
        Completion::Calculation95 => 0x95,
        Completion::Calculation97 => 0x97,
        Completion::Calculation98 => 0x98,
        Completion::Calculation99 => 0x99,
        Completion::Calculation9a => 0x9a,
        Completion::Calculation9b => 0x9b,
        Completion::Calculation9c => 0x9c,
        Completion::Calculation9d => 0x9d,
        Completion::Filea1 => 0xa1,
    }
}

pub fn parameters(bytes: &[u8]) -> Parameters<'_> { Parameters(bytes) }

pub fn client_callback(parameters: Parameters<'_>) -> Result<ClientCallback, ParseError> {
    let bytes: [u8; 4] = parameters.0.get(..4).ok_or(ParseError::MissingCallback)?.try_into().map_err(|_| ParseError::MissingCallback)?;
    Ok(ClientCallback(u32::from_le_bytes(bytes)))
}

pub fn callback_word(callback: ClientCallback) -> u32 { callback.0 }

pub fn optional_callback(word: u32) -> Option<ClientCallback> {
    if word == 0 { None } else { Some(ClientCallback(word)) }
}

pub fn reply_target_code(target: ReplyTarget) -> u8 {
    match target {
        ReplyTarget::Command(kind) => command_code(kind),
        ReplyTarget::Notification(notification) => match notification {
            ClientNotification::Notification12 => 0x12,
            ClientNotification::Notification18 => 0x18,
            ClientNotification::Notification19 => 0x19,
            ClientNotification::Notification1b => 0x1b,
            ClientNotification::Notification1c => 0x1c,
        },
    }
}

pub fn parse(operation: u16, payload: &[u8]) -> Result<Message<'_>, ParseError> {
    let code = operation.checked_sub(0x3b00).and_then(|code| u8::try_from(code).ok()).ok_or(ParseError::UnknownOperation)?;
    if let Some(kind) = command(code) {
        if payload.len() > PARAMETER_BLOCKS[usize::from(code)].capacity { return Err(ParseError::Oversize); }
        Ok(Message::Command(Command { kind, parameters: Parameters(payload) }))
    } else if let Some(completion) = completion(code) {
        Ok(Message::Completed(Completed { completion, parameters: Parameters(payload) }))
    } else {
        Err(ParseError::UnknownOperation)
    }
}

pub fn lens_dc_operation(operation: LensDcOperation) -> u16 {
    match operation {
        LensDcOperation::Request00 => 0x4200,
        LensDcOperation::Request01 => 0x4201,
        LensDcOperation::Request02 => 0x4202,
        LensDcOperation::Request03 => 0x4203,
        LensDcOperation::Request04 => 0x4204,
        LensDcOperation::Request05 => 0x4205,
        LensDcOperation::Request06 => 0x4206,
        LensDcOperation::Request07 => 0x4207,
        LensDcOperation::Request08 => 0x4208,
        LensDcOperation::Request09 => 0x4209,
        LensDcOperation::Request0b => 0x420b,
        LensDcOperation::Request0c => 0x420c,
        LensDcOperation::Request11 => 0x4211,
        LensDcOperation::Request12 => 0x4212,
        LensDcOperation::Request13 => 0x4213,
        LensDcOperation::Request14 => 0x4214,
        LensDcOperation::Request15 => 0x4215,
        LensDcOperation::Request16 => 0x4216,
        LensDcOperation::Request17 => 0x4217,
        LensDcOperation::Request18 => 0x4218,
        LensDcOperation::Request19 => 0x4219,
        LensDcOperation::Request1a => 0x421a,
        LensDcOperation::Request1c => 0x421c,
        LensDcOperation::Request1d => 0x421d,
        LensDcOperation::Request1e => 0x421e,
        LensDcOperation::Request20 => 0x4220,
    }
}

pub fn img_dc_operation(operation: ImgDcOperation) -> u16 {
    match operation {
        ImgDcOperation::Request00 => 0x3f00,
        ImgDcOperation::Request01 => 0x3f01,
        ImgDcOperation::Request13 => 0x3f13,
        ImgDcOperation::Request16 => 0x3f16,
        ImgDcOperation::Request18 => 0x3f18,
        ImgDcOperation::Request1a => 0x3f1a,
        ImgDcOperation::Request1b => 0x3f1b,
        ImgDcOperation::Request1d => 0x3f1d,
        ImgDcOperation::Request1e => 0x3f1e,
        ImgDcOperation::Request1f => 0x3f1f,
        ImgDcOperation::Request20 => 0x3f20,
        ImgDcOperation::Request21 => 0x3f21,
        ImgDcOperation::Request22 => 0x3f22,
        ImgDcOperation::Request23 => 0x3f23,
        ImgDcOperation::Request26 => 0x3f26,
        ImgDcOperation::Request27 => 0x3f27,
        ImgDcOperation::Request28 => 0x3f28,
        ImgDcOperation::Request29 => 0x3f29,
        ImgDcOperation::Request2a => 0x3f2a,
        ImgDcOperation::Request2b => 0x3f2b,
        ImgDcOperation::Request2c => 0x3f2c,
        ImgDcOperation::Request2d => 0x3f2d,
        ImgDcOperation::Request2e => 0x3f2e,
        ImgDcOperation::Request2f => 0x3f2f,
        ImgDcOperation::Request30 => 0x3f30,
        ImgDcOperation::Request31 => 0x3f31,
        ImgDcOperation::Request32 => 0x3f32,
        ImgDcOperation::Request33 => 0x3f33,
        ImgDcOperation::Request34 => 0x3f34,
        ImgDcOperation::Request3a => 0x3f3a,
        ImgDcOperation::Request3b => 0x3f3b,
        ImgDcOperation::Request42 => 0x3f42,
        ImgDcOperation::Request43 => 0x3f43,
        ImgDcOperation::Request44 => 0x3f44,
        ImgDcOperation::Request45 => 0x3f45,
        ImgDcOperation::Request46 => 0x3f46,
        ImgDcOperation::Request47 => 0x3f47,
        ImgDcOperation::Request49 => 0x3f49,
        ImgDcOperation::Request4a => 0x3f4a,
        ImgDcOperation::Request4e => 0x3f4e,
        ImgDcOperation::Request4f => 0x3f4f,
        ImgDcOperation::Request56 => 0x3f56,
        ImgDcOperation::Request57 => 0x3f57,
    }
}

pub fn calc_operation(operation: CalcOperation) -> u16 {
    match operation {
        CalcOperation::Request00 => 0x3c00,
        CalcOperation::Request01 => 0x3c01,
        CalcOperation::Request02 => 0x3c02,
        CalcOperation::Request03 => 0x3c03,
        CalcOperation::Request04 => 0x3c04,
        CalcOperation::Request05 => 0x3c05,
        CalcOperation::Request09 => 0x3c09,
        CalcOperation::Request0a => 0x3c0a,
        CalcOperation::Request0c => 0x3c0c,
        CalcOperation::Request0d => 0x3c0d,
        CalcOperation::Request0e => 0x3c0e,
        CalcOperation::Request0f => 0x3c0f,
        CalcOperation::Request10 => 0x3c10,
        CalcOperation::Request11 => 0x3c11,
        CalcOperation::Request12 => 0x3c12,
    }
}

/// A stock code word a peer enters when it finishes a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockCallbackWord(pub u32);

/// One calc, calc1 or statistics message as the peer receives it. Bytes stock leaves unwritten
/// are zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Encoded { bytes: [u8; 248], length: usize }

impl Encoded {
    pub fn bytes(&self) -> &[u8] { &self.bytes[..self.length] }

    fn new(length: usize) -> Self { Self { bytes: [0; 248], length } }

    fn put(&mut self, offset: usize, value: &[u8]) { self.bytes[offset..offset + value.len()].copy_from_slice(value); }

    fn word(&mut self, offset: usize, value: u32) { self.put(offset, &value.to_le_bytes()); }

    fn halfword(&mut self, offset: usize, value: u16) { self.put(offset, &value.to_le_bytes()); }
}

/// X-M1 1.50: the primary statistics buffer the window calculation reads, as 0xf3a564..0xf3a73c names it.
const WINDOW_STATISTICS: (u32, u32) = (0x5ffe_c1b4, 4096);

/// X-M1 1.50: 0xf601fc (0x3c02), 0xf4c7d0 (0x3c03), 0xf4c82c (0x3c04), 0xf4c8ac (0x3c05),
/// 0xf4c400 (0x3c09), 0xf4c448 (0x3c0a), 0xf4c6e8 (0x3c0c), 0xf4c4d8 (0x3c11) and 0xf4c88c (0x3c12)
/// lay out and size each message they post through 0xf49598.
pub fn calc_payload(payload: &calc::CalcPayload) -> (CalcOperation, Encoded) {
    use calc::CalcPayload as P;
    match *payload {
        P::Calculation { block, source, callback } => {
            let mut message = Encoded::new(232);
            message.word(0, callback.0);
            message.word(4, block.0);
            message.word(8, source.0);
            (CalcOperation::Request02, message)
        }
        P::PipelineFirst { first, second, callback } => {
            let mut message = Encoded::new(232);
            message.word(0, callback.0);
            message.put(4, &first.0);
            message.put(64, &second.0);
            (CalcOperation::Request03, message)
        }
        P::PipelineSecond { block, callback } => (CalcOperation::Request04, callback_first(callback, &block.0)),
        P::PipelineThird { block, callback } => (CalcOperation::Request05, callback_first(callback, &block.0)),
        P::Search { record, callback } => (CalcOperation::Request09, callback_first(callback, &record.0)),
        P::Image { record, callback } => (CalcOperation::Request0a, callback_first(callback, &record.0)),
        P::Window { request, callback } => (CalcOperation::Request0c, window(request, callback)),
        P::Source { request, callback } => (CalcOperation::Request11, source(request, callback)),
        P::Release => (CalcOperation::Request12, Encoded::new(0)),
    }
}

/// X-M1 1.50: 0xf4c584 (0x3d00), 0xf4c620 (0x3d01) and 0xf4c490 (0x3d02) post the whole
/// record followed by the callback through 0xf49c18.
pub fn calc1_payload(payload: &calc::Calc1Payload) -> (Calc1Operation, Encoded) {
    use calc::Calc1Payload as P;
    match *payload {
        P::Acquisition { record, callback } => (Calc1Operation::Request00, callback_last(&record.0, callback)),
        P::Selection { record, callback } => (Calc1Operation::Request01, callback_last(&record.0, callback)),
        P::Auxiliary { request, callback } => (Calc1Operation::Request02, auxiliary(request, callback)),
    }
}

/// X-M1 1.50: 0xf4c584 copies the 1236-byte grid to 0x16d4280 and cleans the first 1204
/// bytes from the data cache (0x740e3c) before posting 0x3d00.
pub const ACQUISITION_GRID: (u32, usize, usize) = (0x016d_4280, 1236, 1204);

fn callback_first(callback: StockCallbackWord, record: &[u8]) -> Encoded {
    let mut message = Encoded::new(4 + record.len());
    message.word(0, callback.0);
    message.put(4, record);
    message
}

fn callback_last(record: &[u8], callback: StockCallbackWord) -> Encoded {
    let mut message = Encoded::new(record.len() + 4);
    message.put(0, record);
    message.word(record.len(), callback.0);
    message
}

/// X-M1 1.50: the auxiliary statistics buffers the 0x3d02 request names (0xf37bcc..0xf37be0).
const AUXILIARY_BUFFERS: [u32; 2] = [0x5ffe_d304, 0x5ffe_e304];

/// X-M1 1.50: 0xf37ab8 fills the 124-byte record at 0x16bfff0 that 0xf38048 zeroes on pass 1
/// and whose pass byte it rewrites on passes 2 and 3; 0xf4c490 appends the callback.
fn auxiliary(request: calc::AuxiliaryRequest, callback: StockCallbackWord) -> Encoded {
    let sample = request.sample;
    let mut message = Encoded::new(128);
    message.word(0x00, u32::from(request.complete));
    message.put(0x04, &[request.pass]);
    message.word(0x08, u32::from(request.selection));
    for (index, word) in request.profile.into_iter().enumerate() { message.word(0x0c + 4 * index, word); }
    message.word(0x1c, sample.gain_step);
    message.halfword(0x22, sample.level);
    message.word(0x24, sample.exposure[0]);
    message.halfword(0x28, request.reference);
    message.halfword(0x2a, sample.offset_level);
    message.word(0x2c, sample.mode);
    message.word(0x30, sample.exposure[1]);
    message.word(0x34, sample.size[0]);
    message.word(0x38, sample.size[1]);
    message.word(0x3c, sample.extent[0]);
    message.word(0x40, sample.extent[1]);
    message.halfword(0x44, 100);
    message.halfword(0x46, 100);
    message.halfword(0x48, 256);
    message.word(0x4c, sample.origin[0]);
    message.word(0x50, sample.origin[1]);
    message.word(0x54, request.rate);
    message.word(0x58, AUXILIARY_BUFFERS[0]);
    message.word(0x5c, AUXILIARY_BUFFERS[1]);
    message.word(0x64, request.level_class);
    message.word(0x68, sample.adjustment);
    message.word(0x6c, u32::from(matches!(request.window_status, 3 | 4)));
    message.halfword(0x70, request.resource_level);
    message.halfword(0x72, request.window_halfword);
    message.word(0x74, request.word);
    message.halfword(0x78, request.halfword);
    message.word(0x7c, callback.0);
    message
}

/// X-M1 1.50: 0xf3a564..0xf3a73c builds the 76-byte record and 0xf4c6e8 appends the
/// callback. Bytes 56..63 are adjustment words +0x48 (0xfa8018) and, for a profile whose
/// 0x74fa2c byte is 6, +0x44 (0xfa9588).
fn window(request: calc::WindowRequest, callback: StockCallbackWord) -> Encoded {
    let mut message = Encoded::new(80);
    message.word(0, u32::from(request.selection));
    message.word(4, request.window.first);
    message.halfword(8, request.level);
    message.word(12, request.sample.mode);
    message.put(16, &[request.sample.flags]);
    message.word(20, request.sample.exposure);
    message.halfword(24, request.sample.level);
    message.halfword(26, request.reference);
    message.halfword(32, request.window.halfword);
    message.word(36, WINDOW_STATISTICS.0);
    message.word(40, WINDOW_STATISTICS.1);
    message.word(44, request.sample.adjustment);
    message.word(48, request.window.status);
    message.word(52, request.window.second);
    message.word(56, request.adjustment[0]);
    message.word(60, request.adjustment[1]);
    message.word(64, u32::from(request.refresh));
    message.word(68, u32::from(request.mode));
    message.word(72, u32::from(request.selected));
    message.word(76, callback.0);
    message
}

/// X-M1 1.50: the statistics buffer the source request names (0xf38730..0xf38738).
const SOURCE_STATISTICS: u32 = 0x5ffe_d304;

/// X-M1 1.50: 0xf386b8 fills the 48-byte record at 0x16c0088 and 0xf4c4f4..0xf4c568 moves
/// its selection word ahead of the reference and the words after the reference after it,
/// leaving message bytes 18..19 and 26..27 unwritten.
fn source(request: calc::SourceRequest, callback: StockCallbackWord) -> Encoded {
    let mut message = Encoded::new(56);
    message.word(0, callback.0);
    message.word(4, SOURCE_STATISTICS);
    message.word(8, request.profile[0]);
    message.halfword(12, request.decade);
    message.halfword(14, request.profile_halfword);
    message.halfword(16, request.level);
    message.word(20, u32::from(request.selection));
    message.halfword(24, request.reference);
    message.word(28, request.size[0]);
    message.word(32, request.size[1]);
    message.word(36, request.profile[1]);
    message.word(40, request.exposure);
    message.word(48, request.mode);
    message.word(52, request.extension);
    message
}

pub fn calc1_operation(operation: Calc1Operation) -> u16 {
    match operation {
        Calc1Operation::Request00 => 0x3d00,
        Calc1Operation::Request01 => 0x3d01,
        Calc1Operation::Request02 => 0x3d02,
    }
}

pub fn statistics_operation(operation: StatisticsOperation) -> u16 {
    match operation {
        StatisticsOperation::Request00 => 0x4000,
        StatisticsOperation::Request01 => 0x4001,
        StatisticsOperation::Request02 => 0x4002,
        StatisticsOperation::Request03 => 0x4003,
        StatisticsOperation::Request04 => 0x4004,
        StatisticsOperation::Request05 => 0x4005,
        StatisticsOperation::Request06 => 0x4006,
        StatisticsOperation::Request07 => 0x4007,
        StatisticsOperation::Request08 => 0x4008,
        StatisticsOperation::Request09 => 0x4009,
        StatisticsOperation::Request0a => 0x400a,
        StatisticsOperation::Request0b => 0x400b,
        StatisticsOperation::Request0c => 0x400c,
        StatisticsOperation::Request0e => 0x400e,
        StatisticsOperation::Request0f => 0x400f,
        StatisticsOperation::Request10 => 0x4010,
        StatisticsOperation::Request11 => 0x4011,
        StatisticsOperation::Request12 => 0x4012,
        StatisticsOperation::Request13 => 0x4013,
        StatisticsOperation::Request14 => 0x4014,
        StatisticsOperation::Request17 => 0x4017,
        StatisticsOperation::Request2a => 0x402a,
        StatisticsOperation::Request2c => 0x402c,
        StatisticsOperation::Request2e => 0x402e,
        StatisticsOperation::Request41 => 0x4041,
        StatisticsOperation::Request43 => 0x4043,
    }
}

pub const fn flash_operation(operation: FlashOperation) -> u16 {
    match operation { FlashOperation::Request0d => 0x1a0d, FlashOperation::Request0e => 0x1a0e }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureRecordField {
    Completion, Auxiliary, Sample, ReservedHeaderA, Progress, ReservedHeaderB,
    PrimarySource, SecondarySource, SourceChannel, SourceDimensions, ReservedSource,
    PolicyDimensions, ReservedPolicy, RecordKind, Channel, Policy, ReservedBeforeAuxiliary,
    AuxiliaryValue, ReservedAfterAuxiliary, Scale, ReservedBeforePosition, Position,
    ReservedAfterPosition, ConvertedDimensions, ReservedBeforePositionCode, PositionCode,
    SourceOption, ReservedOption, Option, PolicyEnabled, MultipleFrames, ReservedMode,
    OrdinaryMode, Enabled, ReservedTail,
}

impl CaptureRecordField {
    pub fn bytes(self, packet: &[u8; 176]) -> &[u8] {
        let range = match self {
            Self::Completion => 0..4, Self::Auxiliary => 4..8, Self::Sample => 8..12,
            Self::ReservedHeaderA => 12..16, Self::Progress => 16..20, Self::ReservedHeaderB => 20..24,
            Self::PrimarySource => 24..40, Self::SecondarySource => 40..56,
            Self::SourceChannel => 56..60, Self::SourceDimensions => 60..64,
            Self::ReservedSource => 64..84, Self::PolicyDimensions => 84..88,
            Self::ReservedPolicy => 88..96, Self::RecordKind => 96..100, Self::Channel => 100..104,
            Self::Policy => 104..108, Self::ReservedBeforeAuxiliary => 108..116,
            Self::AuxiliaryValue => 116..120, Self::ReservedAfterAuxiliary => 120..128,
            Self::Scale => 128..132, Self::ReservedBeforePosition => 132..136,
            Self::Position => 136..138, Self::ReservedAfterPosition => 138..140,
            Self::ConvertedDimensions => 140..150, Self::ReservedBeforePositionCode => 150..161,
            Self::PositionCode => 161..162, Self::SourceOption => 162..163,
            Self::ReservedOption => 163..164, Self::Option => 164..165,
            Self::PolicyEnabled => 165..166, Self::MultipleFrames => 166..167,
            Self::ReservedMode => 167..171, Self::OrdinaryMode => 171..172,
            Self::Enabled => 172..173, Self::ReservedTail => 173..176,
        };
        &packet[range]
    }
}

/// X-M1 1.50: 0xf3292c..0xf329f8 builds this geometry; 0xfea57c..0xfea5b4 sends 40 bytes to imgdc 0x3f2b.
pub fn measurement_geometry(value: crate::round::MeasurementGeometry) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[0..2].copy_from_slice(&[16, 16]);
    bytes[2..4].copy_from_slice(&100u16.to_le_bytes());
    bytes[4..6].copy_from_slice(&100u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&value.resource.0[0].to_le_bytes());
    bytes[12..14].copy_from_slice(&value.resource.0[1].to_le_bytes());
    bytes[14..16].copy_from_slice(&[7, 7]);
    for (offset, calibration) in [18, 16, 22, 20].into_iter().zip(value.calibration.0) {
        bytes[offset..offset + 2].copy_from_slice(&calibration.to_le_bytes());
    }
    bytes[24..26].copy_from_slice(&value.resource.0[0].to_le_bytes());
    bytes[26..28].copy_from_slice(&value.resource.0[1].to_le_bytes());
    bytes[28..32].copy_from_slice(&value.converted_profile.0.to_le_bytes());
    bytes[32..36].copy_from_slice(&value.first_parameter.0.to_le_bytes());
    bytes[36..40].copy_from_slice(&value.second_parameter.0.to_le_bytes());
    bytes
}

/// The largest lens payload, 0x4202's and 0x420b's.
pub const LENS_PAYLOAD_BYTES: usize = 28;

/// The retained callback a lens payload names in its first word; zero for none.
pub const fn lens_callback_word(payload: &crate::lens::LensPayload) -> u32 {
    use crate::lens::{DriveOwner, LensPayload as P, Setting};
    match payload {
        P::BringUp { .. } => 0xf150f0,
        P::Stop { .. } => 0xf150c4,
        P::Setting { setting: Setting::Request04, .. } => 0xf14b50,
        P::Setting { setting: Setting::Request05, .. } => 0xf14b8c,
        P::Setting { setting: Setting::Request06, .. } => 0xf14b14,
        P::Drive { owner: DriveOwner::Live, .. } => 0xf14be4,
        P::Drive { owner: DriveOwner::S1, .. } => 0xf28e94,
        P::Step(_) => 0xf14bc8,
        P::Select { .. } => 0xf14ae4,
        P::Finish { .. } | P::Halt | P::Release => 0,
    }
}

/// Writes a lens payload's wire bytes and returns the posted length. X-M1 1.50 senders:
/// 0x4202 0x76dbe4 (seven words, built at 0xf15148..0xf151a0); 0x4203 0x76dc90 (four words,
/// 0xf151dc..0xf15208); 0x4204/0x4205/0x4206 0x76dd24/0x76ddb0/0x76de3c (three words stored at
/// 0x76dddc..0x76dde4); 0x4207 0x76dec8 (r0..r3 and two stack words, 0x76df04..0x76df20);
/// 0x4208 0x76df7c (three words, r0 zero at 0xf14e4c and 0xf28fc0); 0x4209 0x76e008 and
/// 0x4212 0x770080 (no payload); 0x420b 0x76e1ac (callback, halfword step, zero halfword,
/// mode 0x81a170[0] = 0, then 1, 0, 0, 4 from 0xf14e80..0xf14e9c); 0x4211 0x76ffec (four words,
/// 0x77001c..0x770028, code 11 or 12 from 0xf14fe8..0xf14ff0).
pub fn lens_payload(payload: &crate::lens::LensPayload, bytes: &mut [u8; LENS_PAYLOAD_BYTES]) -> usize {
    use crate::lens::{LensPayload as P, SelectionCode};
    fn put(bytes: &mut [u8; LENS_PAYLOAD_BYTES], words: &[u32]) -> usize {
        for (chunk, word) in bytes.chunks_exact_mut(4).zip(words) {
            chunk.copy_from_slice(&word.to_le_bytes());
        }
        words.len() * 4
    }
    *bytes = [0; LENS_PAYLOAD_BYTES];
    let callback = lens_callback_word(payload);
    match *payload {
        P::BringUp { parameters: p, record } => put(bytes, &[callback, p[0].0, p[1].0, p[2].0, record.0, p[3].0, p[4].0]),
        P::Stop { parameters: p, record } => put(bytes, &[callback, p[0].0, p[1].0, record.0]),
        P::Setting { slot, mode, .. } => put(bytes, &[callback, slot.0, mode.0]),
        P::Drive { parameters: p, slot, .. } => put(bytes, &[callback, p[0].0, p[1].0, p[2].0, p[3].0, slot.0]),
        P::Finish { value, record } => put(bytes, &[callback, value.0, record.0]),
        P::Halt | P::Release => 0,
        P::Step(step) => {
            let length = put(bytes, &[callback, 0, 0, 1, 0, 0, 4]);
            bytes[4..6].copy_from_slice(&step.0.to_le_bytes());
            length
        }
        P::Select { value, code, record } => {
            let code = match code { SelectionCode::Unconfigured => 11, SelectionCode::Configured => 12 };
            put(bytes, &[callback, value.0, code, record.0])
        }
    }
}

/// The interface request a lens payload posts, its bytes in `bytes`.
pub fn lens_dc_request<'a>(payload: &crate::lens::LensPayload, bytes: &'a mut [u8; LENS_PAYLOAD_BYTES]) -> LensDcRequest<'a> {
    let length = lens_payload(payload, bytes);
    LensDcRequest { operation: payload.operation(), parameters: Parameters(&bytes[..length]), completions: payload.completions() }
}
