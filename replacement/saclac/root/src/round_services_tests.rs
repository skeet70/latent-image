//! Synthetic record and kernel fixtures; no stock instructions execute here.

use xm1_focus_measurement_round::{jobs::Effect, window::Window};
use xm1_saclac_interface::{stock::{parameters, StockCallbackWord}, Completed, Completion};
use xm1_saclac_profile_records::Bytes;
use crate::{fake::{self, Entries}, round_services::Callbacks};

#[test]
fn a_window_completion_publishes_position_and_releases_held_power_only_once() {
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
    let mut window = Window::new();
    window.effect(Effect::Submit, record, None, &mut root);
    window.effect(Effect::Submit, record, None, &mut root);
    let result = [3, 0, 0, 0, 0x34, 0x12, 0xcd, 0xab];
    let completion = |kind| Completed { completion: kind, parameters: parameters(&result) };
    window.effect(Effect::Collect, record, Some(completion(Completion::Calculation8a)), &mut root);
    assert_eq!(&root.records().memory().shared[0x734..0x738], &1u32.to_le_bytes());
    window.effect(Effect::Collect, record, Some(completion(Completion::Calculation8d)), &mut root);
    window.effect(Effect::Release, record, None, &mut root);
    window.effect(Effect::Release, record, None, &mut root);
    let shared = &root.records().memory().shared;
    assert_eq!(&shared[0x734..0x738], &3u32.to_le_bytes());
    assert_eq!(&shared[0x838..0x83c], &[0x34, 0x12, 0xcd, 0xab]);
    let power: Vec<_> = machine.take_entered().into_iter().filter(|(entry, _)| *entry == 0x64982c).collect();
    assert_eq!(power, vec![(0x64982c, vec![13, 8, 0]), (0x64982c, vec![13, 0, 0])]);
}
