#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileSelection(pub u16);

pub fn initialize_selection_tables(selection: ProfileSelection, grid: &mut [u8; 16], reference: &mut [u8; 0xbc]) {
    for field in grid.chunks_exact_mut(2) { field.copy_from_slice(&selection.0.to_le_bytes()); }
    for offset in [0, 0x16, 0x18, 0x1a, 0x9c, 0xb4, 0xb6, 0xb8] { reference[offset..offset + 2].copy_from_slice(&selection.0.to_le_bytes()); }
    for offset in [0x1c, 0x1e, 0x20, 0x22, 0x24, 0x26, 0x28, 0x30] { reference[offset..offset + 2].fill(0); }
    reference[0x2c..0x30].fill(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_initialization_retains_unwritten_profile_fields() {
        let mut grid = [0xa5; 16];
        let mut reference = [0x5a; 0xbc];
        initialize_selection_tables(ProfileSelection(0x1234), &mut grid, &mut reference);
        assert_eq!(grid, [0x34, 0x12, 0x34, 0x12, 0x34, 0x12, 0x34, 0x12, 0x34, 0x12, 0x34, 0x12, 0x34, 0x12, 0x34, 0x12]);
        assert_eq!(&reference[0x16..0x20], &[0x34, 0x12, 0x34, 0x12, 0x34, 0x12, 0, 0, 0, 0]);
        assert_eq!(&reference[0xb4..], &[0x34, 0x12, 0x34, 0x12, 0x34, 0x12, 0x5a, 0x5a]);
        for offset in [2, 0x15, 0x2a, 0x32, 0x9e, 0xb3] { assert_eq!(reference[offset], 0x5a); }
        assert_eq!(&reference[0x2c..0x32], &[0; 6]);
    }
}
