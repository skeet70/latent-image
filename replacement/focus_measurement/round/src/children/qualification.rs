//! Qualification view of the S1 acquisition child (issue 117, X-M1 1.50).
//!
//! Machine 24 is the acquisition machine. Root runs the live branch's acquisition in the
//! measurement child and the S1 branch's in `S1Acquisition`; the stock step's state is the
//! one the current `Root::Mode` selects (S1 and SuspendingForS1 are the S1 branch). This view
//! exposes the S1 stage through the acquisition machine's own exhaustive view.

use super::S1Acquisition;
use crate::acquisition::qualification::StageView;

/// The stage the S1 acquisition child holds. `acquisition` is private to this module tree;
/// `crate::acquisition::qualification::stage` matches every `Stage`, so a new stage fails the
/// qualification compile.
pub fn stage(child: &S1Acquisition) -> StageView {
    crate::acquisition::qualification::stage(&child.acquisition)
}
