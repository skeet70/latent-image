//! The group predicate at 0x00f7_e364.

/// The image-5 body at 0x00f7_e364, which rejects a value of -0x8000.
pub fn is_selected_group(kind: u32, selection: u32, value: i32) -> bool {
    if kind == 1 {
        return false;
    }
    if selection != 47 && selection != 56 {
        return false;
    }
    value.wrapping_add(0x8000) != 0
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "C" fn xm1_power_record_helper_f7e364(kind: u32, selection: u32, value: i32) -> u32 {
    u32::from(is_selected_group(kind, selection, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_one_never_selects() {
        assert!(!is_selected_group(1, 47, 0));
        assert!(!is_selected_group(1, 56, 0));
        assert!(!is_selected_group(1, 0, 0));
    }

    #[test]
    fn only_selections_47_and_56_select() {
        assert!(!is_selected_group(0, 46, 0));
        assert!(!is_selected_group(0, 48, 0));
        assert!(!is_selected_group(0, 55, 0));
        assert!(!is_selected_group(0, 57, 0));
        assert!(is_selected_group(0, 47, 0));
        assert!(is_selected_group(2, 56, 0));
    }

    #[test]
    fn the_minimum_halfword_value_does_not_select() {
        assert!(!is_selected_group(0, 47, -0x8000));
        assert!(!is_selected_group(0, 56, -0x8000));
        assert!(is_selected_group(0, 47, -0x7fff));
        assert!(is_selected_group(0, 47, -0x8001));
        assert!(is_selected_group(0, 56, 0x7fff));
    }
}
