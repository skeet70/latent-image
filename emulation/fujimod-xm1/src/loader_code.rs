pub(super) const MUTABLE_TABLE_OFFSETS: [u64; 8] = [0x3a0, 0x3b0, 0x3c0, 0x3d0, 0x3e0, 0x3a0, 0x3f0, 0x400];

pub(super) const fn immutable_loader_spans(base: u64, end: u64) -> [(u64, u64); 8] {
    let mut spans = [(0, 0); 8];
    let mut start = base;
    let mut index = 0;
    while index < 7 {
        let cell = base + MUTABLE_TABLE_OFFSETS[0] + index as u64 * 0x10;
        spans[index] = (start, cell);
        start = cell + 4;
        index += 1;
    }
    spans[7] = (start, end);
    spans
}
