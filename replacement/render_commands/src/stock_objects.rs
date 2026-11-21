//! The stock draw layer's resource-object reference to its VGLIB object.
//!
//! Listing addresses are in `decode/img6.dis` (plain image at `0x01100000`).

use super::resources::DataAddress;

/// Word `+4` of a resource object: the VGLIB object returned by `0x6dce28`/`0x6dcc08` (`117d75c-117d760`,
/// `117d800-117d804`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VglibObject(pub DataAddress);
