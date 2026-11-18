#![forbid(unsafe_code)]

use crate::adjustment::LensPosition;
use crate::stock::{RecordHalfword, RecordWord};
use xm1_saclac_interface::stock::StockCallbackWord;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureMode { Ordinary, ModeSix }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceSlot { Primary, Tertiary }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Source {
    pub identity: RecordWord,
    pub parameters: [RecordWord; 3],
    pub length: RecordWord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Dimensions {
    pub width: RecordHalfword,
    pub height: RecordHalfword,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeightSplit {
    pub height: RecordHalfword,
    pub excess: RecordHalfword,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capture {
    pub channel: RecordWord,
    pub dimensions: Dimensions,
    pub position: LensPosition,
    pub policy: RecordWord,
    pub option: u8,
    pub sources: [Source; 3],
    pub source_option: u8,
    pub exposure: RecordWord,
    pub auxiliary: RecordWord,
    pub mode: CaptureMode,
    pub multiple_frames: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Callbacks {
    pub completion: StockCallbackWord,
    pub sample: StockCallbackWord,
    pub auxiliary: StockCallbackWord,
    pub progress: StockCallbackWord,
}

pub trait Port {
    fn minimum_exposure(&mut self) -> RecordWord;
    fn maximum_exposure(&mut self) -> RecordWord;
    fn policy_dimensions(&mut self, policy: RecordWord) -> Dimensions;
    fn encode_position(&mut self, position: LensPosition) -> u8;
    fn split_height(&mut self, height: RecordHalfword) -> HeightSplit;
    fn policy_enabled(&mut self, policy: RecordWord) -> bool;
    fn setup_source(&mut self, slot: SourceSlot, identity: RecordWord, length: RecordWord);
    fn request_capture(&mut self, packet: &[u8; 176]);
}

pub fn start(input: Capture, callbacks: Callbacks, port: &mut impl Port) {
    if input.mode != CaptureMode::ModeSix {
        let minimum = port.minimum_exposure().0.wrapping_mul(100);
        if minimum > input.exposure.0 {
            port.minimum_exposure();
        } else if input.exposure.0 > port.maximum_exposure().0.wrapping_mul(1_000_000) {
            port.maximum_exposure();
        }
    }
    let dimensions = port.policy_dimensions(input.policy);
    let encoded_position = port.encode_position(input.position);
    let height = port.split_height(input.dimensions.height);
    let total = port.split_height(RecordHalfword(height.height.0.wrapping_add(height.excess.0)));
    let enabled = port.policy_enabled(input.policy);
    for (slot, source) in [(SourceSlot::Primary, input.sources[0]), (SourceSlot::Tertiary, input.sources[2])] {
        if source.identity.0 != 0 {
            port.setup_source(slot, source.identity, source.length);
        }
    }
    port.request_capture(&crate::stock::capture_record_payload(
        input, callbacks, dimensions, encoded_position, height, total, enabled,
    ));
}
