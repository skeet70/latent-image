use super::producer::ProducerError;
use super::DrawReport;

#[test]
fn word_write_refuses_an_out_of_range_offset_without_changing_bytes() {
    let mut report = DrawReport([0xa5; 0x44]);
    assert_eq!(report.put_word(0x41, 0x11223344), Err(ProducerError::ReportRange));
    assert_eq!(report.put_word(usize::MAX, 1), Err(ProducerError::ReportRange));
    assert_eq!(report, DrawReport([0xa5; 0x44]));
    assert_eq!(report.put_word(0x40, 0x11223344), Ok(()));
    assert_eq!(&report.0[0x40..], &[0x44, 0x33, 0x22, 0x11]);
}
