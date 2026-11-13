#![forbid(unsafe_code)]

//! Requests to lenscom that saclac makes through the configuration layer (0x6b731c, 0x6bbe60).

use xm1_saclac_interface::shared::{LensComCode, LensComCommand};
use xm1_saclac_interface::stock::{self, StockCallbackWord};
use xm1_saclac_interface::{LensCom, LensComOperation, LensComRequest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestNumber(u8);

impl RequestNumber {
    pub const fn new(number: u32) -> Option<Self> {
        if number <= 10 { Some(Self(number as u8)) } else { None }
    }

    pub const fn get(self) -> u8 { self.0 }

    /// The code an odd number or 0 posts; even numbers from 2 complete without lenscom.
    const fn posted_code(self) -> Option<u8> {
        match self.0 {
            0 => Some(4),
            1 => Some(0),
            3 => Some(1),
            5 => Some(2),
            7 | 9 => Some(3),
            _ => None,
        }
    }
}

/// The client callback a request completes through. The completion lenscom enters reads it back
/// from the same place.
pub trait LensComClient {
    fn remember(&mut self, callback: Option<StockCallbackWord>, number: RequestNumber);
    /// The word lenscom enters when a posted request finishes.
    fn completion(&self) -> StockCallbackWord;
    fn complete(&mut self, callback: StockCallbackWord, number: RequestNumber);
    fn report_unposted(&mut self);
}

pub fn request<L: LensCom>(
    number: RequestNumber,
    callback: Option<StockCallbackWord>,
    client: &mut impl LensComClient,
    lens_com: &mut L,
) {
    let Some(code) = number.posted_code().and_then(LensComCode::new) else {
        if let Some(callback) = callback {
            client.complete(callback, number);
        }
        return;
    };
    client.remember(callback, number);
    let command = LensComCommand { code, completion: client.completion() };
    let bytes = stock::shared::lens_com(&command);
    let request = LensComRequest { operation: LensComOperation::Request2d, parameters: stock::parameters(&bytes), completions: &[] };
    if lens_com.request(request).is_err() {
        client.report_unposted();
    }
}
