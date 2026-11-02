use fujimod_xm1::{modal_trace::BeginRequest, provenance::Guest};
use serde_json::{Value, json};

const RUNTIME: u32 = 0x0100_0000;
const STOCK: u32 = 0x00bf_b028;
const PUSH: u32 = 0xe92d_4ff0;

fn context_sources() -> Vec<Value> {
    let stores = [
        (0x40240, 0xe582_0000u32), (0x4031c, 0xe582_3000), (0x436b8, 0xe582_1000),
        (0x43690, 0xe586_3000), (0x436f0, 0xe586_3000), (0x43720, 0xe586_3000),
        (0x43848, 0xe583_2000), (0x42440, 0xe583_5000), (0x4dad4, 0xffff_ffff),
        (0xd0240, 0xe582_0000), (0xd031c, 0xe582_3000),
        (0xd36b8, 0xe582_1000), (0xd3690, 0xe586_3000), (0xd36f0, 0xe586_3000),
        (0xd3720, 0xe586_3000), (0xd3848, 0xe583_2000), (0xd2440, 0xe583_5000), (0xdda98, 0xffff_ffff),
    ];
    let mut result = Vec::new();
    for (base, end) in [(0x40000u32, 0x4dae0u32), (0xd0000, 0xddaa0)] {
        let mut start = base;
        for stop in [base + 0x3a0, base + 0x3b0, base + 0x3c0, base + 0x3d0,
            base + 0x3e0, base + 0x3f0, base + 0x400, end] {
            let mut bytes = vec![0; (stop - start) as usize];
            for (pc, word) in stores {
                if start <= pc && pc + 4 <= stop {
                    let offset = (pc - start) as usize;
                    bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
                }
            }
            result.push(json!({"start":start,"bytes":bytes}));
            start = stop + 4;
        }
    }
    result
}

fn request(scoped: bool) -> Value {
    let mut stock = vec![0; 0xfc];
    stock[..4].copy_from_slice(&PUSH.to_le_bytes());
    let text: Vec<_> = [0xe1a0_0000u32; 64].into_iter().flat_map(u32::to_le_bytes).collect();
    let service: Vec<_> = [0xe92d_4010u32, 0xe581_3000, 0xe92d_4010, 0xe581_3000]
        .into_iter().flat_map(u32::to_le_bytes).collect();
    let mut readonly = vec![json!({"start":RUNTIME + 0x200,"bytes":service})];
    if !scoped { readonly.push(json!({"start":RUNTIME,"bytes":text})); }
    let mut publications: Vec<_> = (0..223u32).map(|index| json!({
        "address":0x0120_0000 + index * 4,"old":0,"new":index + 1,
    })).collect();
    if scoped {
        let branch = 0xea00_0000 | (((RUNTIME.wrapping_sub(STOCK).wrapping_sub(8)) >> 2) & 0x00ff_ffff);
        publications.push(json!({"address":STOCK,"old":PUSH,"new":branch}));
    }
    json!({
        "stock":stock,
        "linked":if scoped { json!({"start":RUNTIME,"bytes":text}) } else { Value::Null },
        "seam":if scoped { json!([RUNTIME,RUNTIME + 56]) } else { Value::Null },
        "owned":if scoped { json!([RUNTIME + 0x40,RUNTIME + 0x60]) } else { Value::Null },
        "owned_prehook":if scoped { json!(RUNTIME + 0x80) } else { Value::Null },
        "helpers":if scoped { json!([[RUNTIME + 0x80,RUNTIME + 0x94],[RUNTIME + 0xa0,RUNTIME + 0xb0]]) } else { json!([]) },
        "context_sources":context_sources(),"limit":128,
        "lifetime":{
            "runtime":[RUNTIME,RUNTIME + 0x1000],"resident":[RUNTIME + 0x800,RUNTIME + 0xc00],
            "readonly":readonly,
            "install":{"entry":{"pc":RUNTIME + 0x200,"word":0xe92d_4010u32},"writer":{"pc":RUNTIME + 0x204,"word":0xe581_3000u32}},
            "rollback":{"entry":{"pc":RUNTIME + 0x208,"word":0xe92d_4010u32},"writer":{"pc":RUNTIME + 0x20c,"word":0xe581_3000u32}},
            "publications":publications,
        },
    })
}

fn accepted(value: Value) -> bool {
    serde_json::from_value::<BeginRequest>(value).unwrap().into_config(Vec::new())
        .is_ok_and(|config| config.owns_memory_watch())
}

#[test]
fn complete_stock_and_scoped_source_declarations_own_the_lifetime_watch() {
    assert!(accepted(request(false)));
    assert!(accepted(request(true)));
}

#[test]
fn every_context_span_and_its_actual_store_words_are_required() {
    for case in 0..6 {
        let mut value = request(true);
        match case {
            0 => { value["context_sources"].as_array_mut().unwrap().pop(); }
            1 => { value["context_sources"][0]["bytes"].as_array_mut().unwrap().pop(); }
            2 => value["context_sources"][0]["start"] = json!(0x40004),
            3 => value["context_sources"][0]["bytes"][(0x40240 - 0x40000) as usize] = json!(1),
            4 => value["context_sources"][7]["bytes"][(0x42440 - 0x40404) as usize] = json!(1),
            _ => value["context_sources"][15]["bytes"][(0xdda98 - 0xd0404) as usize] = json!(0),
        }
        assert!(!accepted(value));
    }
}

#[test]
fn complete_helper_ranges_and_their_source_placement_are_required() {
    for case in 0..5 {
        let mut value = request(true);
        match case {
            0 => value["helpers"] = json!([]),
            1 => value["helpers"][1] = value["helpers"][0].clone(),
            2 => value["owned_prehook"] = json!(RUNTIME + 0x84),
            3 => value["helpers"][1] = json!([RUNTIME + 0x100,RUNTIME + 0x110]),
            _ => value["lifetime"]["runtime"][0] = json!(RUNTIME + 4),
        }
        assert!(!accepted(value));
    }
}

#[test]
fn only_the_exact_selected_entry_publication_may_change_bound_code() {
    for case in 0..4 {
        let mut value = request(true);
        match case {
            0 => value["lifetime"]["publications"][0]["address"] = json!(RUNTIME + 0x40),
            1 => value["lifetime"]["publications"][223]["new"] = json!(0xea00_0000u32),
            2 => value["lifetime"]["publications"][223]["old"] = json!(0xe1a0_0000u32),
            _ => value["lifetime"]["publications"][0]["address"] = json!(0x40240),
        }
        assert!(!accepted(value));
    }
    let mut baseline = request(false);
    baseline["lifetime"]["publications"][0]["address"] = json!(Guest(STOCK).0);
    assert!(!accepted(baseline));
}

#[test]
fn lifetime_source_payloads_are_bounded_and_readonly_sections_are_disjoint() {
    let mut large = request(false);
    large["lifetime"]["runtime"][1] = json!(RUNTIME + (1 << 20) + 4);
    assert!(!accepted(large));
    let mut many = request(false);
    let image = many["lifetime"]["readonly"][0].clone();
    many["lifetime"]["readonly"] = json!(vec![image; 65]);
    assert!(!accepted(many));
    let mut overlap = request(false);
    let image = overlap["lifetime"]["readonly"][0].clone();
    overlap["lifetime"]["readonly"].as_array_mut().unwrap().push(image);
    assert!(!accepted(overlap));
}
