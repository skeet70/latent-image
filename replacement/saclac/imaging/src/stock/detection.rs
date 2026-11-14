//! The detection placements' stock side: the detector result 0x10451ac copies, the crop
//! request and geometry of 0xef6030, the centring 0xedb0dc, and the callers' output words.

use super::capture::StockCapture;
use crate::capture::detection::{
    place_detection as place, place_grown_detection as place_grown, rescale_detection as rescale, Crop, CropRequest, Detection, DetectionPorts, Extent, Framing,
    Placement, Point, Rect, Visible,
};
use crate::capture::ports::Adjustments;
use crate::capture::AdjustmentKey;

/// The words of the 0x42c-byte buffer 0x10451ac fills from its first twelve.
const RESULT_WORDS: usize = 0x42c / 4;
/// The words 0x10517a0 copies from the buffer's third.
pub const COPIED_WORDS: usize = 0x420 / 4;
/// 0x84, the status a refused placement returns.
pub const REFUSED: u32 = 0x84;

#[derive(Clone, Copy, Debug, Default)]
pub struct StockDetection;

fn result_words() -> [u32; RESULT_WORDS] {
    let mut words = [0u32; RESULT_WORDS];
    entry!(0x104_51ac, fn(*mut u32))(words.as_mut_ptr());
    words
}

fn detection_words(detection: &Detection) -> [u32; RESULT_WORDS] {
    let mut words = [0u32; RESULT_WORDS];
    let Detection { frame, status, state, reserved, count, corner, size, center } = *detection;
    words[..12].copy_from_slice(&[frame[0], frame[1], status, state, reserved, count, corner.x, corner.y, size.y, size.x, center.x, center.y]);
    words
}

fn crop_words(crop: &Crop) -> [u32; 8] {
    let Crop { origin, frame, window } = *crop;
    [origin.x, origin.y, frame[0], frame[1], window.origin.x, window.origin.y, window.extent.height, window.extent.width]
}

const fn rect(words: [u32; 4]) -> Rect { Rect { origin: Point { x: words[0], y: words[1] }, extent: Extent { height: words[2], width: words[3] } } }

impl Adjustments for StockDetection {
    fn byte(&self, key: AdjustmentKey) -> u8 { StockCapture.byte(key) }
}

impl DetectionPorts for StockDetection {
    fn detection(&mut self) -> Detection {
        let w = result_words();
        Detection {
            frame: [w[0], w[1]],
            status: w[2],
            state: w[3],
            reserved: w[4],
            count: w[5],
            corner: Point { x: w[6], y: w[7] },
            size: Point { x: w[9], y: w[8] },
            center: Point { x: w[10], y: w[11] },
        }
    }

    fn crop(&mut self, request: &CropRequest) -> Crop {
        let ratio = u32::from(request.ratio[0]) | u32::from(request.ratio[1]) << 16;
        let words = [request.profile, request.area[0], request.area[1], request.area[2], ratio, u32::from(request.inset)];
        let mut out = [0u32; 8];
        entry!(0xef_6030, fn(*const u32, *mut u32))(words.as_ptr(), out.as_mut_ptr());
        Crop { origin: Point { x: out[0], y: out[1] }, frame: [out[2], out[3]], window: rect([out[4], out[5], out[6], out[7]]) }
    }

    fn centered(&mut self, crop: &Crop, detection: &Detection) -> Rect {
        let crop = crop_words(crop);
        let result = detection_words(detection);
        let mut out = [0u32; 4];
        entry!(0xed_b0dc, fn(*const u32, *const u32, *mut u32))(crop.as_ptr(), result.as_ptr(), out.as_mut_ptr());
        rect(out)
    }
}

/// Writes the status, state and box words a placement leaves in the caller's output; word 2 is
/// not written, and without a box only the count (word 3) is cleared.
fn write_placement(placement: &Placement, out: &mut [u32; 10]) {
    out[0] = placement.status;
    out[1] = placement.state;
    match placement.placed {
        Some(placed) => out[3..].copy_from_slice(&[
            placed.count,
            placed.corner.x,
            placed.corner.y,
            placed.extent.height,
            placed.extent.width,
            placed.center.x,
            placed.center.y,
        ]),
        None => out[3] = 0,
    }
}

fn write_framing(framing: &Framing, out: &mut [u32; 10]) {
    let Framing { frame, offset, extent } = *framing;
    let half = [offset.x, offset.y, extent.height, extent.width];
    out[..2].copy_from_slice(&frame);
    out[2..6].copy_from_slice(&half);
    out[6..].copy_from_slice(&half);
}

fn finish(result: Result<(Placement, Option<Framing>), crate::capture::detection::Refused>, out: &mut [u32; 10], framing: Option<&mut [u32; 10]>) -> u32 {
    let Ok((placement, used)) = result else { return REFUSED };
    write_placement(&placement, out);
    if let (Some(used), Some(framing)) = (used, framing) {
        write_framing(&used, framing);
    }
    0
}

/// 0x10510e8's call: the caller's area block and output words; `None` for a null pointer.
pub fn place_detection(profile: u32, area: Option<&[u32; 3]>, out: Option<&mut [u32; 10]>, framing: Option<&mut [u32; 10]>) -> u32 {
    let (Some(area), Some(out)) = (area, out) else { return REFUSED };
    finish(place(&mut StockDetection, profile, *area), out, framing)
}

/// 0x10512cc's call: the area block's halfword +0xe is the frame mode.
pub fn place_grown_detection(profile: u32, area: Option<&[u32; 4]>, out: Option<&mut [u32; 10]>, framing: Option<&mut [u32; 10]>) -> u32 {
    let (Some(area), Some(out)) = (area, out) else { return REFUSED };
    let visible = Visible::from_mode((area[3] >> 16) as u16);
    finish(place_grown(&mut StockDetection, profile, [area[0], area[1], area[2]], visible), out, framing)
}

/// 0x10515f4's call: the area block's fourth word is the second crop's ratio, `to` the height
/// and width the box is rescaled to.
pub fn rescale_detection(area: Option<&[u32; 4]>, to: Option<&[u32; 2]>, out: Option<&mut [u32; 10]>) -> u32 {
    let (Some(area), Some(to), Some(out)) = (area, to, out) else { return REFUSED };
    let ratio = [area[3] as u16, (area[3] >> 16) as u16];
    let placement = rescale(&mut StockDetection, [area[0], area[1], area[2]], ratio, Extent { height: to[0], width: to[1] });
    write_placement(&placement, out);
    0
}

/// 0x10517a0's call: the detector result from its status on, the rest zero.
pub fn copy_detection(out: Option<&mut [u32; COPIED_WORDS]>) -> u32 {
    let Some(out) = out else { return REFUSED };
    let detection = StockDetection.detection();
    out.copy_from_slice(&detection_words(&detection)[2..2 + COPIED_WORDS]);
    0
}
