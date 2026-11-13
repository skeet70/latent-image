#![forbid(unsafe_code)]

//! What the DCF profile geometry layer sends the image pipeline's stage control.

/// The format stage's region: the profile's two pair scales and its colour-pattern alignment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatPayload {
    pub first_scale: u16,
    pub second_scale: u16,
    pub alignment: [u8; 2],
    /// The alignment comes from the class remainders rather than the profile class.
    pub remainder_geometry: bool,
}
