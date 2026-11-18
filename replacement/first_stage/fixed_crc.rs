const POLYNOMIAL: u32 = 0xedb8_8320;

const fn entry(mut crc: u32) -> u32 {
    let mut bit = 0;
    while bit < 8 {
        crc = (crc >> 1) ^ if crc & 1 == 0 { 0 } else { POLYNOMIAL };
        bit += 1;
    }
    crc
}

const fn table() -> [u32; 256] {
    let mut entries = [0; 256];
    let mut index = 0;
    while index < entries.len() {
        entries[index] = entry(index as u32);
        index += 1;
    }
    entries
}

const TABLE: [u32; 256] = table();

#[inline(always)]
pub fn update(crc: u32, value: u8) -> u32 {
    (crc >> 8) ^ TABLE[((crc as u8) ^ value) as usize]
}

#[cfg(test)]
mod tests {
    use super::update;

    fn checksum(bytes: &[u8]) -> u32 {
        !bytes.iter().fold(0xffff_ffff, |crc, byte| update(crc, *byte))
    }

    #[test]
    fn standard_vector() {
        assert_eq!(checksum(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn empty_input() {
        assert_eq!(checksum(b""), 0);
    }
}
