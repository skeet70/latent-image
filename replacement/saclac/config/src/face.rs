#![forbid(unsafe_code)]

//! The face-tracking entries saclac's modules call (image 5, 0x106631c..0x106d02c). The face
//! tracking pipeline and history are tasks 3's and 55's state, so the entries that touch them
//! stay stock; saclac owns the calls and the image processor request.

use xm1_saclac_interface::shared::ImageProcessorTriplet;
use xm1_saclac_interface::stock::shared::image_processor_triplet;
use xm1_saclac_interface::stock::{parameters, StockCallbackWord};
use xm1_saclac_interface::{ImageProcessor, ImageProcessorOperation, ImageProcessorRequest};

/// The pipeline was neither idle nor stopped, so detection was not reset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrackingBusy;

/// The face-tracking entries, which stay stock.
pub trait FaceTracking {
    /// The caller's geometry and output blocks, in the layout the tracking command reads.
    type Geometry;
    type Output;

    /// 0x106d02c, which 0x106637c forwards to.
    fn initialize(&mut self) -> Result<(), TrackingBusy>;
    /// 0x106a2f0, which 0x106638c forwards to; the stock status.
    fn command(&mut self, mode: u32, geometry: &Self::Geometry, output: &mut Self::Output) -> i32;
}

/// The image processor refused the request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NotPosted;

/// Posts `values` with `completion` to the image processor (0x106631c, which returns -255 for
/// `NotPosted`).
pub fn submit_triplet<P: ImageProcessor>(processor: &mut P, values: [u32; 3], completion: StockCallbackWord) -> Result<(), NotPosted> {
    let bytes = image_processor_triplet(&ImageProcessorTriplet { completion, values });
    let request = ImageProcessorRequest { operation: ImageProcessorOperation::Request03, parameters: parameters(&bytes) };
    processor.request(request).map_err(|_| NotPosted)
}
