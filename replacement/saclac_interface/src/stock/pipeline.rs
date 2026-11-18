#![forbid(unsafe_code)]

//! X-M1 1.50 bytes of the half-press pipeline's requests and of the results its completion
//! callbacks deliver. Stock callbacks store the results into shot memory before they post
//! (0xf205c0, 0xf20594, 0xf2056c, 0xf20380, 0xf20418, 0xf203b4); the owned callbacks carry them as
//! the posted payload instead. Request blocks leave the bytes stock never writes zero; the ports
//! append or prepend the callback words.

use crate::pipeline::{
    CaptureArea, FormatPayload, FrameRequest, GradationCalculation, GradationRequest, RawCalculation, RawCheck, RawFrameRequest, ScanRequest,
    ScanWindow, SelectionRequest, StatisticsSetup, StatisticsWeights, SubjectGeometry, ToneCalculation, ToneProgress, ToneRequest,
    ToneStatus, GRADATION_CALCULATION_BYTES, RAW_CALCULATION_BYTES, SCAN_AREA_BYTES, SCAN_WINDOWS_BYTES, TONE_CALCULATION_BYTES,
};

struct Block<const BYTES: usize>([u8; BYTES]);

impl<const BYTES: usize> Block<BYTES> {
    fn new() -> Self { Self([0; BYTES]) }
    fn put(&mut self, offset: usize, value: &[u8]) -> &mut Self {
        self.0[offset..offset + value.len()].copy_from_slice(value);
        self
    }
    fn word(&mut self, offset: usize, value: u32) -> &mut Self { self.put(offset, &value.to_le_bytes()) }
    fn half(&mut self, offset: usize, value: u16) -> &mut Self { self.put(offset, &value.to_le_bytes()) }
    fn words(&mut self, offset: usize, values: &[u32]) -> &mut Self {
        for (index, value) in values.iter().enumerate() { self.word(offset + 4 * index, *value); }
        self
    }
}

fn subject(block: &mut Block<52>, subject: &SubjectGeometry) {
    block.word(0x00, subject.profile).word(0x04, subject.state).half(0x08, subject.horizontal).half(0x0a, subject.vertical);
}

fn area<const BYTES: usize>(block: &mut Block<BYTES>, offset: usize, area: &CaptureArea) {
    block.word(offset, area.reference).words(offset + 4, &area.areas).word(offset + 0x10, area.area);
}

pub const RAW_FRAME_REQUEST_BYTES: usize = 48;

/// The block 0xf20a88 builds for imgdc 0x3f1d; 0xfea8e8 appends the callback word.
pub fn raw_frame_request(request: &RawFrameRequest) -> [u8; RAW_FRAME_REQUEST_BYTES] {
    let mut whole = Block::<52>::new();
    subject(&mut whole, &request.subject);
    whole.word(0x0c, request.subject.extent);
    area(&mut whole, 0x10, &request.area);
    whole.put(0x24, &[100, 100, 16]).word(0x28, request.image_buffer).word(0x2c, request.raw_word);
    let mut bytes = [0; RAW_FRAME_REQUEST_BYTES];
    bytes.copy_from_slice(&whole.0[..RAW_FRAME_REQUEST_BYTES]);
    bytes
}

pub const FRAME_REQUEST_BYTES: usize = 52;

/// The block 0xf209e4 builds for imgdc 0x3f1e; 0xfea91c appends the callback word.
pub fn frame_request(request: &FrameRequest) -> [u8; FRAME_REQUEST_BYTES] {
    let mut block = Block::<FRAME_REQUEST_BYTES>::new();
    subject(&mut block, &request.subject);
    block.half(0x0c, u16::from(request.kind)).word(0x10, request.subject.extent);
    area(&mut block, 0x14, &request.area);
    block.word(0x28, request.image_buffer).word(0x2c, request.sampling).put(0x30, &[request.mode]);
    block.0
}

pub const SCAN_REQUEST_BYTES: usize = 120;

/// Context bytes 0x16b54c0..0x16b5538, which 0xf2230c fills and 0xf21ae4 hands to 0xfea950 for
/// imgdc 0x3f1f; 0xfea950 appends the 0x51 then the 0x50 callback word.
pub fn scan_request(request: &ScanRequest) -> [u8; SCAN_REQUEST_BYTES] {
    let mut block = Block::<SCAN_REQUEST_BYTES>::new();
    block.word(0x00, request.start).half(0x04, request.positions[0]).half(0x06, request.positions[1]).half(0x08, request.positions[2]);
    block.word(0x0c, request.selection_word).half(0x10, request.span).half(0x12, request.index_base);
    block.word(0x14, request.weighting).word(0x18, request.tolerance_kind);
    match request.window {
        ScanWindow::None => {}
        ScanWindow::Area(area) => { block.put(0x1c, &area[..SCAN_AREA_BYTES]); }
        ScanWindow::Centred { centre, first, second, held } => {
            block.words(0x1c, &centre).words(0x24, &second).words(0x2c, &first).words(0x34, &[1, 1, u32::from(held)]);
        }
    }
    block.word(0x40, request.selection_setting).put(0x44, &request.windows[..SCAN_WINDOWS_BYTES]).half(0x74, request.correction);
    block.0
}

/// 0xfea9f8's eight bytes for imgdc 0x3f20: the word, then the zero byte 0xf220ec passes.
pub fn selection_request(request: SelectionRequest) -> [u8; 8] {
    let mut block = Block::<8>::new();
    block.word(0, request.0);
    block.0
}

pub const TONE_REQUEST_BYTES: usize = 64;

/// The block 0xf20ec0 builds for calc 0x3c04 (0xf4c82c).
pub fn tone_request(request: &ToneRequest) -> [u8; TONE_REQUEST_BYTES] {
    let mut block = Block::<TONE_REQUEST_BYTES>::new();
    for (index, level) in request.levels.iter().enumerate() { block.half(2 * index, *level as u16); }
    block.half(0x22, request.offset).word(0x24, 1).words(0x28, &request.window);
    block.0
}

pub const GRADATION_REQUEST_BYTES: usize = 116;

/// The block 0xf21074 builds for calc 0x3c05 (0xf4c8ac).
pub fn gradation_request(request: &GradationRequest) -> [u8; GRADATION_REQUEST_BYTES] {
    let mut block = Block::<GRADATION_REQUEST_BYTES>::new();
    block.put(0, &request.tone.0);
    if request.gated { block.word(0x04, 1); }
    block.word(0x6c, u32::from(request.sampled)).half(0x70, request.exposure);
    block.0
}

/// The five blocks 0xf20b60 hands to 0x1039798 for statistics 0x03, with the format and kind
/// arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatisticsBlocks {
    pub selection: [u8; 16],
    pub format: u32,
    pub area: [u8; 16],
    pub levels: [u8; 16],
    pub sampling: [u8; 56],
    pub capture: [u8; 56],
    pub kind: u32,
}

pub fn statistics_blocks(setup: &StatisticsSetup) -> StatisticsBlocks {
    let mut selection = Block::<16>::new();
    selection.words(0, &setup.selection).word(0x0c, 1);
    let mut levels = Block::<16>::new();
    levels.words(0, &setup.levels);
    let sampled = &setup.sampling;
    let mut sampling = Block::<56>::new();
    sampling.words(0x00, &sampled.capture[..4]).word(0x10, 1).words(0x14, &sampled.capture[4..]).words(0x20, &sampled.lens);
    for (index, half) in sampled.halves.iter().enumerate() { sampling.half(0x28 + 2 * index, *half); }
    sampling.word(0x30, sampled.sample).put(0x34, &sampled.flags);
    let mut capture = Block::<56>::new();
    capture.words(0, &setup.capture.words).put(0x1d, &setup.capture.flags).half(0x34, setup.capture.layout);
    StatisticsBlocks {
        selection: selection.0,
        format: setup.mode,
        area: setup.area,
        levels: levels.0,
        sampling: sampling.0,
        capture: capture.0,
        kind: if setup.weighted { 2 } else { 0 },
    }
}

/// The scalar arguments to0x1042e9c, after the target handle and fourteen-byte table pointer.
pub fn statistics_weights_arguments(weights: &StatisticsWeights) -> [u32; 7] {
    [u32::from(weights.mode), u32::from(weights.extent[0]), u32::from(weights.extent[1]), u32::from(weights.origin[0]), u32::from(weights.origin[1]),
        u32::from(weights.size[0]), u32::from(weights.size[1])]
}

fn word(payload: &[u8], offset: usize) -> Option<u32> { Some(u32::from_le_bytes(payload.get(offset..offset + 4)?.try_into().ok()?)) }

/// 0xf205c0 stores its first argument.
pub fn raw_check_payload(check: RawCheck) -> [u8; 4] { check.0.to_le_bytes() }

pub fn raw_check(payload: &[u8]) -> Option<RawCheck> { word(payload, 0).map(RawCheck) }

/// 0xf20594 stores its first argument as the status and the halfword of its second as the level.
pub fn tone_progress_payload(progress: ToneProgress) -> [u8; 6] {
    let mut bytes = [0; 6];
    bytes[..4].copy_from_slice(&progress.status.to_le_bytes());
    bytes[4..].copy_from_slice(&progress.level.to_le_bytes());
    bytes
}

pub fn tone_progress(payload: &[u8]) -> Option<ToneProgress> {
    Some(ToneProgress { status: word(payload, 0)?, level: u16::from_le_bytes(payload.get(4..6)?.try_into().ok()?) })
}

/// 0xf2056c stores its first argument.
pub fn tone_status_payload(status: ToneStatus) -> [u8; 4] { status.0.to_le_bytes() }

pub fn tone_status(payload: &[u8]) -> Option<ToneStatus> { word(payload, 0).map(ToneStatus) }

/// 0xf20380 copies 20 bytes from its argument.
pub fn raw_calculation(payload: &[u8]) -> Option<RawCalculation> { Some(RawCalculation(payload.get(..RAW_CALCULATION_BYTES)?.try_into().ok()?)) }

/// 0xf20418 copies 108 bytes from its argument.
pub fn tone_calculation(payload: &[u8]) -> Option<ToneCalculation> { Some(ToneCalculation(payload.get(..TONE_CALCULATION_BYTES)?.try_into().ok()?)) }

/// 0xf203b4 copies 96 bytes from its argument.
pub fn gradation_calculation(payload: &[u8]) -> Option<GradationCalculation> {
    Some(GradationCalculation(payload.get(..GRADATION_CALCULATION_BYTES)?.try_into().ok()?))
}

pub const FORMAT_BLOCK_BYTES: usize = 0x34;

/// The block 0xee2d28 builds on its stack and passes to 0xda027c with flag 4: 0x685b94 reads the
/// three halfwords at +0x24, 0x685dd0 the scales and alignment at +0x2a, 0x65f3d8 the word at
/// +0x30. Bytes 0..0x24, which stock leaves uninitialised, are zero.
pub fn format_block(payload: &FormatPayload) -> [u8; FORMAT_BLOCK_BYTES] {
    let mut bytes = [0; FORMAT_BLOCK_BYTES];
    bytes[0x2a..0x2c].copy_from_slice(&payload.first_scale.to_le_bytes());
    bytes[0x2c..0x2e].copy_from_slice(&payload.second_scale.to_le_bytes());
    bytes[0x2e..0x30].copy_from_slice(&payload.alignment);
    bytes[0x30..0x34].copy_from_slice(&u32::from(payload.remainder_geometry).to_le_bytes());
    bytes
}
