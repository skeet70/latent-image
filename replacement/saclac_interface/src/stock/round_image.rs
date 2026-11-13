#![forbid(unsafe_code)]

//! X-M1 1.50 bytes of the image lane's imgdc requests and of the completions that finish them.

use crate::round_image::ImageLaneRequest;
use crate::stock::StockCallbackWord;
use crate::ImgDcOperation;

const MESSAGE_BYTES: usize = 20;

/// One image-lane request as imgdc receives it. Bytes stock leaves unwritten are zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageLaneMessage { bytes: [u8; MESSAGE_BYTES], length: usize }

impl ImageLaneMessage {
    pub fn bytes(&self) -> &[u8] { &self.bytes[..self.length] }

    fn new(length: usize) -> Self { Self { bytes: [0; MESSAGE_BYTES], length } }

    fn put(&mut self, offset: usize, value: &[u8]) { self.bytes[offset..offset + value.len()].copy_from_slice(value); }
}

/// `finished` is the word imgdc enters when an image or snapshot finishes; cancel and stop carry
/// none. 0xfea6c8 posts 0x3f30 (20 bytes), 0xfea760 posts 0x3f33 (16 bytes, 10..11 unwritten),
/// 0xfea738 posts 0x3f32 and 0xfea70c posts 0x3f31 (4 bytes each).
pub fn image_lane_request(request: &ImageLaneRequest, finished: StockCallbackWord) -> (ImgDcOperation, ImageLaneMessage) {
    match *request {
        ImageLaneRequest::Image { arguments, area, channel } => {
            let mut message = ImageLaneMessage::new(20);
            message.put(0, &finished.0.to_le_bytes());
            message.put(4, &arguments.0[0].to_le_bytes());
            message.put(8, &arguments.0[1].to_le_bytes());
            message.put(12, &area.0[0].to_le_bytes());
            message.put(14, &area.0[1].to_le_bytes());
            message.put(16, &channel.0.to_le_bytes());
            (ImgDcOperation::Request30, message)
        }
        ImageLaneRequest::Snapshot { argument, area, channel } => {
            let mut message = ImageLaneMessage::new(16);
            message.put(0, &finished.0.to_le_bytes());
            message.put(4, &argument.0.to_le_bytes());
            message.put(6, &area.0[0].to_le_bytes());
            message.put(8, &area.0[1].to_le_bytes());
            message.put(12, &channel.0.to_le_bytes());
            (ImgDcOperation::Request33, message)
        }
        ImageLaneRequest::Cancel(argument) => {
            let mut message = ImageLaneMessage::new(4);
            message.put(0, &argument.0.to_le_bytes());
            (ImgDcOperation::Request32, message)
        }
        ImageLaneRequest::Stop => (ImgDcOperation::Request31, ImageLaneMessage::new(4)),
    }
}

/// The callbacks 0xf39b64 (completion 0x47) and 0xf39b3c (0x66) post imgdc's status word first;
/// 0xf05f60, 0xf05fa8, 0xf06060 and 0xf060a8 report a failed image for any nonzero status.
pub fn image_failed(payload: &[u8]) -> bool {
    payload.get(..4).map_or(false, |status| status != [0; 4])
}

/// 0xf39bf4..0xf39bfc enters the image client's callback with these arguments. 0xf39b8c passes 0
/// first when 0x756ddc reads 1 or 2, which no reachable store writes: the only store of 2 is
/// 0xf39e7c in 0xf39c74, and 0xf39c5c, which gates it, returns 0.
pub const IMAGE_CLIENT_START: [u32; 2] = [2, 0];
