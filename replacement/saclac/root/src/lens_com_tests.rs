use xm1_kernel::fake::Fake as KernelFake;
use xm1_kernel::{Kernel, PostError, TaskId};
use xm1_saclac_config::services::{lens_request, NOT_POSTED, REFUSED_INDEX};
use xm1_saclac_profile_records::Bytes;

use crate::fake::{peers, posts, root_with, saclac_kernel, Entries};

const CALLBACK_WORD: u32 = 0x96_bd04;
const CLIENT: u32 = 0x8e_1f3c;
const COMPLETION: u32 = 0x6b_6f9c;
const UNPOSTED_REPORT: u32 = 0x6b_92cc;
const LENS_COM: u8 = 98;
const CALLBACK: u32 = 0x00c0_ffee;

/// The 48 bytes lenscom receives for `code`, completing through 0x6b6f9c.
fn posted(code: u8) -> Vec<u8> {
    let mut bytes = vec![0; 48];
    bytes[0..4].copy_from_slice(&COMPLETION.to_le_bytes());
    bytes[4..7].copy_from_slice(&[0x40, 0x2a, code]);
    bytes[44] = 1;
    bytes
}

#[test]
fn an_odd_index_posts_its_code_to_lenscom_and_keeps_the_callback_for_the_completion() {
    let kernel = saclac_kernel();
    let cx = kernel.task().unwrap();
    let (fake, mut root) = root_with(peers(&cx), Bytes::default(), Entries::default());
    assert_eq!(lens_request(&mut root, 7, CALLBACK), 0);
    assert_eq!(posts(&kernel), [(LENS_COM, 0x722d, posted(3))]);
    assert_eq!([CALLBACK_WORD, CLIENT + 8, CLIENT + 0xc].map(|address| fake.word(address)), [CALLBACK, CALLBACK, 7]);
    assert_eq!(fake.take_entered(), []);
}

#[test]
fn number_zero_posts_code_four() {
    let kernel = saclac_kernel();
    let cx = kernel.task().unwrap();
    let (_, mut root) = root_with(peers(&cx), Bytes::default(), Entries::default());
    assert_eq!(lens_request(&mut root, 0, CALLBACK), 0);
    assert_eq!(posts(&kernel), [(LENS_COM, 0x722d, posted(4))]);
}

#[test]
fn an_even_number_completes_through_the_callback_without_lenscom() {
    let kernel = saclac_kernel();
    let cx = kernel.task().unwrap();
    let (fake, mut root) = root_with(peers(&cx), Bytes::default(), Entries::default());
    assert_eq!(lens_request(&mut root, 4, CALLBACK), 0);
    assert_eq!(lens_request(&mut root, 10, 0), 0);
    assert_eq!(posts(&kernel), []);
    assert_eq!(fake.take_entered(), [(CALLBACK, vec![4])]);
    assert_eq!(fake.word(CALLBACK_WORD), 0);
}

#[test]
fn a_number_above_ten_is_not_requested_and_an_index_above_fourteen_is_refused() {
    let kernel = saclac_kernel();
    let cx = kernel.task().unwrap();
    let (fake, mut root) = root_with(peers(&cx), Bytes::default(), Entries::default());
    assert_eq!(lens_request(&mut root, 11, CALLBACK), NOT_POSTED);
    assert_eq!(fake.word(CALLBACK_WORD), CALLBACK);
    assert_eq!(lens_request(&mut root, 15, CALLBACK + 1), REFUSED_INDEX);
    assert_eq!(fake.word(CALLBACK_WORD), CALLBACK + 1);
    assert_eq!(posts(&kernel), []);
    assert_eq!(fake.take_entered(), []);
}

#[test]
fn a_post_the_kernel_refuses_is_reported_and_still_counts_as_requested() {
    let kernel = Kernel::new(KernelFake::with_task(TaskId::new(43).unwrap()).post_result(Err(PostError::NoBlock)));
    let cx = kernel.task().unwrap();
    let (fake, mut root) = root_with(peers(&cx), Bytes::default(), Entries::default());
    assert_eq!(lens_request(&mut root, 1, CALLBACK), 0);
    assert_eq!(posts(&kernel).len(), 1);
    assert_eq!(fake.take_entered(), [(UNPOSTED_REPORT, vec![1, 0x27, 0])]);
}
