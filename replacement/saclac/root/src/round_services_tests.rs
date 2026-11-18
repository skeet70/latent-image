//! Synthetic record and kernel fixtures; no stock instructions execute here.

use xm1_focus_measurement_round::{jobs::Effect, window::Window};
use xm1_saclac_interface::{stock::{parameters, StockCallbackWord}, Completed, Completion};
use xm1_saclac_profile_records::Bytes;
use crate::{fake::{self, Entries}, round_services::Callbacks};

#[test]
fn a_window_completion_publishes_the_result_record_and_releases_held_power_only_once() {
    let kernel = fake::saclac_kernel();
    let context = kernel.task().unwrap();
    let mut bytes = Bytes::default();
    let record = fake::open(&mut bytes, &[0, 13, 20, 27]);
    bytes.shared[0x734..0x738].copy_from_slice(&1u32.to_le_bytes());
    let (machine, mut root) = fake::root_with(fake::peers(&context), bytes, Entries::default());
    root.bind_round_callbacks(Callbacks {
        search_mode: StockCallbackWord(0x1000), window: StockCallbackWord(0x1004),
        auxiliary: StockCallbackWord(0x1008), source: StockCallbackWord(0x100c),
        image: StockCallbackWord(0x1010), snapshot: StockCallbackWord(0x1014),
    });
    // calc's result record: status 3, position halfwords 0x1234 and 0xabcd at +0x404/+0x406.
    let result = 0x0200_0000u32;
    machine.set_word(result, 3);
    machine.set_word(result + 0x404, 0xabcd_1234);
    let payload = |record: u32| {
        let mut bytes = [0u8; 8];
        bytes[4..].copy_from_slice(&record.to_le_bytes());
        bytes
    };
    let (named, null) = (payload(result), payload(0));
    let mut window = Window::new();
    window.effect(Effect::Submit, record, None, &mut root);
    window.effect(Effect::Submit, record, None, &mut root);
    window.effect(Effect::Collect, record, Some(Completed { completion: Completion::Calculation8a, parameters: parameters(&named) }), &mut root);
    window.effect(Effect::Collect, record, Some(Completed { completion: Completion::Calculation8d, parameters: parameters(&null) }), &mut root);
    assert_eq!(&root.records().memory().shared[0x734..0x738], &1u32.to_le_bytes());
    window.effect(Effect::Collect, record, Some(Completed { completion: Completion::Calculation8d, parameters: parameters(&named) }), &mut root);
    window.effect(Effect::Release, record, None, &mut root);
    window.effect(Effect::Release, record, None, &mut root);
    let shared = &root.records().memory().shared;
    assert_eq!(&shared[0x734..0x738], &3u32.to_le_bytes());
    assert_eq!(&shared[0x838..0x83c], &[0x34, 0x12, 0xcd, 0xab]);
    let power: Vec<_> = machine.take_entered().into_iter().filter(|(entry, _)| *entry == 0x64982c).collect();
    assert_eq!(power, vec![(0x64982c, vec![13, 8, 0]), (0x64982c, vec![13, 0, 0])]);
}
