use crate::{session::{Session, Tick}, session_abi::{observe, ControlObservation}, transport::PublishedRegion};

#[repr(C)]
pub struct ProbeInput {
    pub frames: *const [u8; 12],
    pub count: u32,
    pub tick: u32,
    pub region_base: u32,
    pub region_length: u32,
}

#[repr(C)]
pub struct ProbeObservation {
    pub result: u32,
    pub control: ControlObservation,
    pub payload_address: u32,
}

/// # Safety
/// Input names an initialized descriptor and count readable frames. Output names count exclusive aligned records, disjoint from input and frames.
#[no_mangle]
#[link_section = ".text.vendor_session_probe"]
pub unsafe extern "C" fn vendor_session_probe(input: *const ProbeInput, output: *mut ProbeObservation) -> u32 {
    let input = &*input;
    let region = if input.region_length == 0 {
        None
    } else {
        match PublishedRegion::new(input.region_base, input.region_length) {
            Ok(region) => Some(region),
            Err(_) => return 1,
        }
    };
    let mut control = [0; 256];
    let mut session = Session::new(&mut control);
    for index in 0..input.count as usize {
        let mut observation = ControlObservation::new();
        let result = match observe(&mut session, &*input.frames.add(index), Tick(input.tick), region, &[], &mut observation) {
            Ok(()) => 0,
            Err(crate::session::FrameError::InvalidPublishedSpan) => 1,
            Err(crate::session::FrameError::MissingPublishedRegion) => 2,
            Err(crate::session::FrameError::ReservedFlags) => 3,
            Err(crate::session::FrameError::Truncated) => 4,
            Err(crate::session::FrameError::ReplyCapacity) => 5,
            Err(crate::session::FrameError::ReplyNotReady) => 6,
        };
        let payload_address = session.payload_span().map_or(0, |span| span.address());
        output.add(index).write(ProbeObservation { result, control: observation, payload_address });
    }
    0
}
