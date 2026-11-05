use fujimod_emulator_devices::parts::nand::Nand;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct RowAddress(pub u32);

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(try_from = "u32")]
pub struct RowCount(u32);

impl TryFrom<u32> for RowCount {
    type Error = String;
    fn try_from(count: u32) -> Result<Self, Self::Error> {
        if (1..=64).contains(&count) { Ok(Self(count)) } else { Err("NAND read count must be 1..=64 rows".into()) }
    }
}

impl RowCount {
    pub fn get(self) -> u32 { self.0 }
}

#[derive(Debug, PartialEq, Eq)]
pub struct NandRead {
    pub first: RowAddress,
    pub count: RowCount,
    pub data: Vec<u8>,
    pub spare: Vec<u8>,
}

pub fn read(nand: &Nand, first: RowAddress, count: RowCount) -> Result<NandRead, String> {
    let options = nand.options();
    let end = first.0.checked_add(count.0).ok_or("NAND read row range overflows")?;
    if u64::from(end) > options.bytes / options.page_bytes as u64 {
        return Err("NAND read row range exceeds medium".into());
    }
    let mut data = vec![0; count.0 as usize * options.page_bytes];
    let mut spare = vec![0; count.0 as usize * options.spare_bytes];
    nand.read(u64::from(first.0) * options.page_bytes as u64, &mut data)?;
    for (row, bytes) in (first.0..end).zip(spare.chunks_exact_mut(options.spare_bytes)) {
        nand.read_spare(row, bytes)?;
    }
    Ok(NandRead { first, count, data, spare })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fujimod_emulator_devices::parts::nand::{Options, Overlay};
    use std::{collections::BTreeMap, sync::Arc};

    fn medium() -> Nand {
        Nand::new(Options { bytes: 2048 * 64, page_bytes: 2048, pages_per_block: 64, spare_bytes: 512 },
            Arc::from(vec![0xff; 2048 * 64]),
            vec![Overlay { offset: 2048, bytes: Arc::from(vec![0x66; 2048]) }],
            BTreeMap::from([(1, vec![0x77; 512])])).unwrap()
    }

    #[test]
    fn reads_inputs_and_committed_program_without_changing_medium() {
        let mut nand = medium();
        nand.program(2, &vec![0x12; 2048], &vec![0x34; 512]).unwrap();
        let before = nand.fingerprint();
        let result = read(&nand, RowAddress(1), RowCount::try_from(2).unwrap()).unwrap();
        assert_eq!(&result.data[..2048], &vec![0x66; 2048]);
        assert_eq!(&result.data[2048..], &vec![0x12; 2048]);
        assert_eq!(&result.spare[..512], &vec![0x77; 512]);
        assert_eq!(&result.spare[512..], &vec![0x34; 512]);
        assert_eq!(nand.fingerprint(), before);
    }

    #[test]
    fn erased_rows_return_erased_data_and_spare() {
        let mut nand = medium();
        nand.erase(0).unwrap();
        let result = read(&nand, RowAddress(1), RowCount::try_from(1).unwrap()).unwrap();
        assert!(result.data.iter().chain(&result.spare).all(|byte| *byte == 0xff));
    }

    #[test]
    fn refuses_invalid_counts_and_row_ranges() {
        assert!(RowCount::try_from(0).is_err());
        assert!(RowCount::try_from(65).is_err());
        let count = RowCount::try_from(2).unwrap();
        assert!(read(&medium(), RowAddress(63), count).is_err());
        assert!(read(&medium(), RowAddress(u32::MAX), count).is_err());
    }
}
