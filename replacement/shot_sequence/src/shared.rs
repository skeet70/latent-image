#![forbid(unsafe_code)]

//! Helpers more than one shot step uses.

/// The capture slot a step works on: the requested slot when it is one of at most three counted
/// slots, else the first.
pub const fn active_slot(requested: u32, count: u32) -> u32 {
    if requested == 0 || count == 0 || count > 3 || requested > count { 1 } else { requested }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_requested_slot_holds_only_within_three_counted_slots() {
        let cases = [((2, 3), 2), ((3, 3), 3), ((1, 1), 1), ((0, 3), 1), ((2, 0), 1), ((4, 4), 1), ((3, 2), 1)];
        for ((requested, count), slot) in cases { assert_eq!(active_slot(requested, count), slot, "{requested} of {count}"); }
    }
}
