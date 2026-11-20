//! Qualification view of the lens children (issue 117, X-M1 1.50).
//!
//! Only the live child's state is part of the stock map (machine 1). The view conversion
//! is exhaustive over the child's private state through `live::qualification`.

use super::LensChildren;
use crate::live::qualification::StateView;

/// The live child's state.
pub fn live_state(children: &LensChildren) -> StateView { crate::live::qualification::view(&children.live) }
