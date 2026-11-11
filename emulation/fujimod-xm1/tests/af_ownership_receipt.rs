use fujimod_xm1::{af_ownership::Plan, af_ownership_receipt::{validate, validate_plan}};
use serde_json::{Value, json};
use fujimod_emulator::board::factory::MemoryMap;

fn expected_mappings() -> Vec<MemoryMap> {
    (0..2).flat_map(|core| [0,0x4000_0000].map(move |address|
        MemoryMap {core,address,physical:0,size:0x0200_0000,permissions:7})).collect()
}

const PC: u32 = 0x0010_0000;
const WORD: u32 = 0xe590_0000;

fn identity() -> Value {
    json!({"sdk":"/nix/store/immutable-sdk", "viewer":"viewer-sha256", "bridge":"bridge-sha256",
        "model":"run-sha256", "package":"manifest-sha256"})
}
fn plan() -> Plan {
    let mut sites = vec![json!({"pc":PC,"word":WORD,"kind":"consumer",
        "reads":[[0x016f_ec4cu32,4]], "register_reads":[{"register":13,"offset":-4,"bytes":4}]})];
    for (pc,kind) in [(0x0072_503cu32,"queue-post"),(0x0072_5318,"dequeue"),(0x0102_8b54,"select"),
        (0x0102_8bc4,"dispatch-callback"),(0x0102_dec8,"callback"),(0x0102_f1b0,"reserve-primary"),
        (0x0102_f1c8,"reserve-secondary"),(0x0102_f1d8,"reserve-secondary"),(0x0103_1664,"primary"),
        (0x0103_a254,"request"),(0x0103_e988,"mode-helper"),(0x0103_fd88,"consumer"),(0x0104_01f0,"current"),
        (0x0104_0250,"set"),(0x0104_0288,"clear"),(0x0105_44e4,"work-post"),(0x0105_47b8,"callback")] {
        sites.push(json!({"pc":pc,"word":WORD,"kind":kind,"reads":[],"register_reads":[]}));
    }
    serde_json::from_value(json!({"identity":identity(),"sites":sites})).expect("valid caller plan")
}
fn words() -> Vec<Value> {
    [0x016f_ec3cu32, 0x016f_ec4c].into_iter()
        .chain((0x016f_ed28..0x016f_ee48).step_by(4))
        .chain([0x0170_306c,0x0170_3080,0x0170_3084,0x0170_30d0,0x0170_3160,0x0170_319c,0x0170_31a4,0x0170_327c])
        .map(|address|json!({"address":address,"value":0})).collect()
}
fn row(address: u32, value: u32, core: u8, instructions: u64, own: u64) -> Value {
    json!({"af_sequence":core,"source":"cpu-write","address":address,"physical":address,"size":4,"core":core,"pc":PC,
        "instructions":instructions,"tick":instructions,"before":[0,0,0,0],"value":value,"byte_order":"little",
        "core_instructions":own,"latest_fetch":{"source":"cpu-fetch","address":PC,"physical":PC,"size":4,
            "core":core,"pc":PC,"instructions":instructions,"tick":instructions,"value":null,"before":null,
            "byte_order":"little","core_instructions":own}})
}
fn context(counts: [u64;2], cursor: usize) -> Value {
    json!({"sequence":cursor,"core":0,"pc":PC,"word":WORD,"kind":"consumer","cpsr":0x13,"registers":vec![0;13],
        "entry":{"opaque_id":format!("InstructionEntryId {{ fixture: {cursor} }}"),"address":PC,"bytes":4,"progress":counts[0],"pstate":0x13,"physical":PC,"tcm":[null,null]},
        "sp":0x0170_1000u32,"lr":0x0010_0004u32,"counts":counts,"cursor":cursor,"sampled_task":{"status":"refused","error":"NullCurrent","context_depth":null},
        "reads":[{"address":0x016f_ec4cu32,"provenance":{"kind":"shared-physical","physical":0x016f_ec4cu32,"observed_tcm":[[0,0],[0,0]]},"bytes":[0,0,0,0]},
            {"address":0x0170_0ffcu32,"provenance":{"kind":"shared-physical","physical":0x0170_0ffcu32,"observed_tcm":[[0,0],[0,0]]},"bytes":[0,0,0,0]}]})
}
fn capture() -> Value {
    let initial = words();
    let mut final_words = initial.clone();
    final_words[0]["value"] = json!(1);
    final_words[1]["value"] = json!(2);
    let mappings:Vec<_>=(0..2).flat_map(|core|[0u64,0x4000_0000].map(move |address|
        json!({"core":core,"address":address,"physical":0,"size":0x0200_0000u32,"permissions":7}))).collect();
    json!({"schema":4,"mode":"passive","retention":{"status":"stopped","reason":"end"},"status":"complete","identity":identity(),"row_count":2,"context_count":2,
        "initial":{"counts":[100,200],"words":initial},"final":{"counts":[104,204],"words":final_words},
        "rows":[row(0x016f_ec3c,1,0,302,101),row(0x016f_ec4c,2,1,304,202)],
        "contexts":[context([101,200],0),context([102,201],1)],"failure":null,"read_refusal":null,
        "mappings":mappings,"limits":{"rows":65536,"contexts":8192,"pairs":128,"output_bytes":134217728}})
}
fn check(capture: &Value) -> Result<(), String> {
    validate(&serde_json::to_vec(capture).expect("serialize capture"), &identity(), &plan(), &expected_mappings())
}
fn refuses(mutate: impl FnOnce(&mut Value)) {
    let mut receipt = capture();
    mutate(&mut receipt);
    assert!(check(&receipt).is_err());
}

#[test]
fn accepts_complete_byte_bound_capture() {
    assert!(check(&capture()).is_ok());
}

#[test]
fn accepts_resolved_decoder_writer_and_rejects_wrong_mapping_provenance() {
    let mut receipt=capture();
    let mut maps=expected_mappings();
    maps.push(MemoryMap {core:0,address:0x8000_0000,physical:u64::from(PC),size:4096,permissions:5});
    receipt["mappings"].as_array_mut().unwrap().push(json!({
        "core":0,"address":0x8000_0000u32,"physical":PC,"size":4096,"permissions":5
    }));
    receipt["rows"][0]["pc"]=json!(0x8000_0000u32);
    receipt["rows"][0]["latest_fetch"]["pc"]=json!(0x8000_0000u32);
    receipt["rows"][0]["latest_fetch"]["address"]=json!(0x8000_0000u32);
    assert!(validate(&serde_json::to_vec(&receipt).unwrap(),&identity(),&plan(),&maps).is_ok());
    receipt["rows"][0]["latest_fetch"]["physical"]=json!(PC+4);
    assert!(validate(&serde_json::to_vec(&receipt).unwrap(),&identity(),&plan(),&maps).is_err());
    refuses(|receipt|receipt["mappings"]=json!([]));
    refuses(|receipt|receipt["mappings"][0]["physical"]=json!(4));
    refuses(|receipt| {
        receipt["mappings"][0]["physical"]=json!(4);
        receipt["rows"][0]["latest_fetch"]["physical"]=json!(PC+4);
    });
    refuses(|receipt|receipt["mappings"][0]["permissions"]=json!(1));
    refuses(|receipt|receipt["mappings"][0]["unrecognized"]=json!(0));
    refuses(|receipt|receipt["mappings"][0]["size"]=json!(u64::MAX));
    refuses(|receipt| {
        let map=receipt["mappings"][0].clone();
        receipt["mappings"]=json!(vec![map;4097]);
    });
}

#[test]
fn refuses_missing_evidence() {
    for key in ["schema","mode","retention","status","identity","initial","final","rows","contexts","failure","read_refusal","limits","row_count","context_count","mappings"] {
        refuses(|receipt| { receipt.as_object_mut().expect("object").remove(key); });
    }
    for key in ["sequence","core","pc","word","kind","cpsr","registers","sp","lr","counts","cursor","reads"] {
        refuses(|receipt| { receipt["contexts"][0].as_object_mut().expect("context").remove(key); });
    }
    for key in ["af_sequence","source","address","physical","size","core","pc","instructions","tick","before","value","byte_order","core_instructions","latest_fetch"] {
        refuses(|receipt| { receipt["rows"][0].as_object_mut().expect("row").remove(key); });
    }
    for key in ["source","address","physical","size","core","pc","instructions","tick","before","value","byte_order","core_instructions"] {
        refuses(|receipt| { receipt["rows"][0]["latest_fetch"].as_object_mut().expect("fetch").remove(key); });
    }
    refuses(|receipt| receipt["rows"] = json!([]));
    refuses(|receipt| receipt["contexts"] = json!([]));
    refuses(|receipt| receipt["final"] = Value::Null);
}

#[test]
fn refuses_identity_status_and_schema_changes() {
    refuses(|receipt| receipt["identity"]["package"] = json!("another-package"));
    refuses(|receipt| receipt["identity"]["extra"] = json!("unbound"));
    refuses(|receipt| receipt["schema"] = json!(1));
    refuses(|receipt| receipt["status"] = json!("armed"));
    refuses(|receipt| receipt["status"] = json!("blocked"));
    refuses(|receipt| receipt["failure"] = json!("overflow"));
    refuses(|receipt| receipt["extra"] = json!(true));
    let bytes = serde_json::to_vec(&capture()).expect("serialize capture");
    assert!(validate(&bytes, &json!({}), &plan(), &expected_mappings()).is_err());
    let mut wrong_plan = plan();
    wrong_plan.identity["sdk"] = json!("another-sdk");
    assert!(validate(&bytes, &identity(), &wrong_plan, &expected_mappings()).is_err());
}

#[test]
fn refuses_incomplete_or_reordered_boundary_words() {
    for boundary in ["initial", "final"] {
        refuses(|receipt| { receipt[boundary]["words"].as_array_mut().expect("words").pop(); });
        refuses(|receipt| receipt[boundary]["words"].as_array_mut().expect("words").swap(0,1));
        refuses(|receipt| receipt[boundary]["words"][2]["address"] = json!(0x016f_ed29u32));
        refuses(|receipt| receipt[boundary]["words"][2]["value"] = json!(u64::from(u32::MAX)+1));
    }
    refuses(|receipt| receipt["final"]["words"][0]["value"] = json!(9));
    refuses(|receipt| receipt["initial"]["words"][0]["value"] = json!(9));
    refuses(|receipt| receipt["final"]["counts"] = json!([99,204]));
    refuses(|receipt| receipt["final"]["counts"] = json!([104,199]));
    refuses(|receipt| receipt["initial"]["counts"] = json!([u64::MAX,1]));
    refuses(|receipt| receipt["final"]["counts"] = json!([u64::MAX,1]));
}

#[test]
fn refuses_event_order_scope_and_provenance_changes() {
    refuses(|receipt| receipt["rows"].as_array_mut().expect("rows").swap(0,1));
    refuses(|receipt| receipt["rows"][0]["instructions"] = json!(299));
    refuses(|receipt| receipt["rows"][1]["instructions"] = json!(309));
    refuses(|receipt| receipt["rows"][0]["physical"] = json!(0x416f_ec3cu32));
    refuses(|receipt| receipt["rows"][0]["size"] = json!(0));
    refuses(|receipt| receipt["rows"][0]["address"] = json!(u32::MAX));
    refuses(|receipt| receipt["rows"][0]["source"] = json!("cpu-read"));
    refuses(|receipt| receipt["rows"][0]["before"] = json!([1,0,0,0]));
    refuses(|receipt| receipt["rows"][0]["value"] = json!(1u64 << 32));
    refuses(|receipt| receipt["rows"][0]["byte_order"] = json!("big"));
    refuses(|receipt| receipt["rows"][0]["core"] = json!(2));
    for (key,value) in [("pc",json!(PC+4)),("physical",json!(PC+4)),("size",json!(2)),
        ("core",json!(1)),("instructions",json!(303)),("tick",json!(303)),
        ("core_instructions",json!(102)),("byte_order",json!("big")),("value",json!(WORD))] {
        refuses(|receipt| receipt["rows"][0]["latest_fetch"][key] = value);
    }
    refuses(|receipt| receipt["rows"][0]["latest_fetch"] = Value::Null);
}

#[test]
fn refuses_context_order_state_bindings_and_read_masks() {
    refuses(|receipt| receipt["contexts"].as_array_mut().expect("contexts").swap(0,1));
    refuses(|receipt| receipt["contexts"][0]["counts"] = json!([99,202]));
    refuses(|receipt| receipt["contexts"][0]["cursor"] = json!(3));
    refuses(|receipt| receipt["contexts"][0]["cursor"] = json!(1));
    refuses(|receipt| receipt["contexts"][1]["cursor"] = json!(0));
    refuses(|receipt| receipt["contexts"][0]["pc"] = json!(PC+2));
    refuses(|receipt| receipt["contexts"][0]["word"] = json!(WORD+1));
    refuses(|receipt| receipt["contexts"][0]["kind"] = json!("set"));
    for mask in [1u32 << 5,1 << 9,1 << 24] {
        refuses(|receipt| receipt["contexts"][0]["cpsr"] = json!(0x13 | mask));
    }
    refuses(|receipt| receipt["contexts"][0]["registers"] = json!(vec![0;12]));
    refuses(|receipt| receipt["contexts"][0]["reads"].as_array_mut().expect("reads").swap(0,1));
    refuses(|receipt| receipt["contexts"][0]["reads"][0]["bytes"] = json!([0,0,0]));
    refuses(|receipt| receipt["contexts"][0]["reads"][0]["address"] = json!(0x016f_ec3cu32));
    refuses(|receipt| receipt["contexts"][0]["reads"][1]["address"] = json!(0x0170_1000u32));
    refuses(|receipt| receipt["contexts"][0]["sp"] = json!(0));
    refuses(|receipt| receipt["contexts"][0]["reads"] = json!([]));
}

#[test]
fn refuses_capacity_and_numeric_overflows() {
    for key in ["rows","contexts","pairs","output_bytes"] {
        refuses(|receipt| receipt["limits"][key] = json!(1));
    }
    refuses(|receipt| receipt["rows"] = Value::Array(vec![receipt["rows"][0].clone();65537]));
    refuses(|receipt| receipt["contexts"] = Value::Array(vec![receipt["contexts"][0].clone();8193]));
    refuses(|receipt| receipt["contexts"][0]["reads"][0]["bytes"] = json!(vec![0;4097]));
    refuses(|receipt| receipt["contexts"][0]["cursor"] = json!(u64::MAX));
    refuses(|receipt| receipt["contexts"][0]["counts"] = json!([u64::MAX,1]));
    let bytes = serde_json::to_vec(&capture()).expect("serialize capture");
    assert!(validate(&bytes[..bytes.len()-1], &identity(), &plan(), &expected_mappings()).is_err());
    let mut extra = bytes;
    extra.extend_from_slice(b"{}");
    assert!(validate(&extra, &identity(), &plan(), &expected_mappings()).is_err());
}

#[test]
fn interrupt_masking_is_not_an_a32_refusal() {
    let mut receipt = capture();
    receipt["contexts"][0]["cpsr"] = json!(0xd3);
    receipt["contexts"][0]["entry"]["pstate"] = json!(0xd3);
    assert!(check(&receipt).is_ok());
}

#[test]
fn refuses_unbound_plans_and_pair_overflow() {
    let bytes = serde_json::to_vec(&capture()).expect("serialize capture");
    let mut expected = plan();
    expected.sites.clear();
    assert!(validate(&bytes, &identity(), &expected, &expected_mappings()).is_err());
    let mut expected = plan();
    expected.sites[0].pc += 2;
    assert!(validate(&bytes, &identity(), &expected, &expected_mappings()).is_err());
    let mut expected = plan();
    expected.sites[0].register_reads[0].register = 15;
    assert!(validate(&bytes, &identity(), &expected, &expected_mappings()).is_err());
    let mut expected = plan();
    expected.sites[0].reads[0] = [u32::MAX, 4];
    assert!(validate(&bytes, &identity(), &expected, &expected_mappings()).is_err());
    let expected = plan();
    let mut receipt = capture();
    let mut entry = receipt["contexts"][0].clone();
    entry["kind"] = json!("reserve-primary");
    entry["pc"] = json!(0x0102_f1b0u32);
    entry["entry"]["address"] = json!(0x0102_f1b0u32);
    entry["entry"]["physical"] = json!(0x0102_f1b0u32);
    entry["reads"] = json!([]);
    receipt["context_count"] = json!(128);
    receipt["contexts"] = Value::Array((0..128).map(|sequence| {
        let mut context = entry.clone();
        context["sequence"] = json!(sequence);
        context
    }).collect());
    assert!(validate(&serde_json::to_vec(&receipt).expect("serialize capture"), &identity(), &expected, &expected_mappings()).is_ok());
    receipt["context_count"] = json!(129);
    entry["sequence"] = json!(128);
    receipt["contexts"].as_array_mut().expect("contexts").push(entry);
    assert!(validate(&serde_json::to_vec(&receipt).expect("serialize capture"), &identity(), &expected, &expected_mappings()).is_err());
}

#[test]
fn accepts_explicit_non_cpu_write_provenance() {
    for source in ["host-write", "model-bus-write"] {
        let mut receipt = capture();
        let row = receipt["rows"][0].as_object_mut().expect("row");
        row.insert("source".into(), json!(source));
        row.insert("core".into(), Value::Null);
        row.insert("pc".into(), Value::Null);
        row.remove("core_instructions");
        row.remove("latest_fetch");
        assert!(check(&receipt).is_ok());
    }
}

#[test]
fn refuses_decreasing_ticks_even_with_matching_fetch() {
    refuses(|receipt| {
        receipt["rows"][1]["tick"] = json!(301);
        receipt["rows"][1]["latest_fetch"]["tick"] = json!(301);
    });
}

#[test]
fn refuses_output_above_the_wire_bound() {
    let bytes = vec![b' ';134_217_729];
    assert!(validate(&bytes, &identity(), &plan(), &expected_mappings()).is_err());
}

#[test]
fn validates_plan_without_guest_evidence() {
    assert!(validate_plan(&plan()).is_ok());
    for missing in 1..plan().sites.len() {
        let mut expected = plan();
        expected.sites.remove(missing);
        assert!(validate_plan(&expected).is_err());
    }
    let mut expected = plan();
    expected.sites.swap(0,1);
    assert!(validate_plan(&expected).is_err());
    let mut expected = plan();
    expected.sites[0].reads = vec![[0x1000,4];17];
    assert!(validate_plan(&expected).is_err());
    let mut expected = plan();
    expected.sites[0].register_reads[0].bytes = 4097;
    assert!(validate_plan(&expected).is_err());
}

#[test]
fn refuses_actual_unclocked_sdk_transfer_rows() {
    for source in ["host-write", "model-bus-write"] {
        let mut receipt = capture();
        receipt["rows"][0] = json!({"af_sequence":0,"source":source,"address":0x016f_ec3cu32,"physical":0x016f_ec3cu32,
            "size":4,"core":null,"pc":null,"instructions":null,"tick":null});
        assert!(check(&receipt).is_err());
    }
}

#[test]
fn accepts_bound_alias_reads_and_actual_alias_write_provenance() {
    let mut expected = plan();
    expected.sites[0].reads[0][0] |= 0x4000_0000;
    let mut receipt = capture();
    for context in receipt["contexts"].as_array_mut().expect("contexts") {
        context["reads"][0]["address"] = json!(0x416f_ec4cu32);
        context["sp"] = json!(0x4170_1000u32);
        context["reads"][1]["address"] = json!(0x4170_0ffcu32);
    }
    receipt["rows"][0]["address"] = json!(0x416f_ec3cu32);
    receipt["rows"][0]["pc"] = json!(PC | 0x4000_0000);
    receipt["rows"][0]["latest_fetch"]["pc"] = json!(PC | 0x4000_0000);
    receipt["rows"][0]["latest_fetch"]["address"] = json!(PC | 0x4000_0000);
    assert!(validate(&serde_json::to_vec(&receipt).expect("serialize capture"), &identity(), &expected, &expected_mappings()).is_ok());
    receipt["contexts"][0]["reads"][0]["physical"] = json!(0x416f_ec4cu32);
    assert!(validate(&serde_json::to_vec(&receipt).expect("serialize capture"), &identity(), &expected, &expected_mappings()).is_err());
}

#[test]
fn refuses_missing_tail_and_discontinuous_sequences() {
    refuses(|receipt| { receipt["contexts"].as_array_mut().expect("contexts").pop(); });
    refuses(|receipt| { receipt["rows"].as_array_mut().expect("rows").pop(); });
    refuses(|receipt| receipt["contexts"][1]["sequence"] = json!(0));
    refuses(|receipt| receipt["rows"][1]["af_sequence"] = json!(0));
    refuses(|receipt| receipt["context_count"] = json!(1));
    refuses(|receipt| receipt["row_count"] = json!(1));
}

#[test]
fn refuses_read_fault_even_with_complete_status_and_cleared_error_string() {
    let detail=json!({
        "core":"primary","pc":PC,"counts":[102,201],
        "origin":{"kind":"register","index":0,"register":13,"base":0x0170_1000u32,"offset":-4},
        "address":0x0170_0ffcu32,"computed_address":0x0170_0ffcu32,"bytes":4,"purpose":"executing-core","execution":{"entry":{"opaque_id":"InstructionEntryId { fixture: 0 }","address":PC,"bytes":4,"progress":102,"pstate":0x12,"physical":PC,"tcm":[null,null]},"cpsr":0x12,"registers":vec![0;13],"sp":0x0170_1000u32,"lr":PC,"sampled_task":{"status":"refused","error":"NullCurrent","context_depth":3}},
        "refusal":{"error":"translation","view":{"kind":"executing-core","core":"primary"},
            "observed_tcm":[[0,0],[0,0]]},
        "live_mappings":{"status":"retained","mappings":capture()["mappings"]}
    });
    let typed:fujimod_xm1::af_ownership::ReadRefusal=serde_json::from_value(detail.clone()).unwrap();
    assert!(typed.validate(&plan()).is_ok());
    assert_eq!(serde_json::to_value(typed).unwrap(),detail);
    refuses(|receipt|receipt["read_refusal"]=detail.clone());
    for key in ["core","pc","counts","origin","address","computed_address","bytes","purpose","execution","refusal","live_mappings"] {
        let mut invalid=detail.clone();
        invalid.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<fujimod_xm1::af_ownership::ReadRefusal>(invalid).is_err(),"{key}");
    }
    for mutate in [
        ("bytes",json!(0)),("bytes",json!(4097)),("core",json!("third")),
        ("counts",json!([102])),("unknown",json!(true)),
    ] {
        let mut invalid=detail.clone();
        invalid[mutate.0]=mutate.1;
        assert!(serde_json::from_value::<fujimod_xm1::af_ownership::ReadRefusal>(invalid).is_err());
    }
    for (key,value) in [("register",json!(15)),("index",json!(128))] {
        let mut invalid=detail.clone();
        invalid["origin"][key]=value;
        assert!(serde_json::from_value::<fujimod_xm1::af_ownership::ReadRefusal>(invalid).is_err());
    }
    let mut invalid=detail.clone();
    let map=invalid["live_mappings"]["mappings"][0].clone();
    invalid["live_mappings"]["mappings"]=json!(vec![map;4097]);
    assert!(serde_json::from_value::<fujimod_xm1::af_ownership::ReadRefusal>(invalid).is_err());
}

#[test]
fn accepts_tcm_payload_provenance_without_invented_physical_address() {
    let mut receipt=capture();
    for context in receipt["contexts"].as_array_mut().unwrap() {
        context["entry"]["tcm"]=json!([null,{"address":0x7000_0000u32,"bytes":0x2000}]);
        context["reads"][0]["provenance"]["observed_tcm"]=json!([[0,0x7000_0011u32],[0,0]]);
        context["sp"]=json!(0x7000_1fdcu32);
        context["reads"][1]=json!({"address":0x7000_1fd8u32,"bytes":[1,2,3,4],
            "provenance":{"kind":"tcm","core":"primary","bank":"atcm","register":0x7000_0011u32,
                "window_start":0x7000_0000u32,"window_end":0x7000_2000u32,"offset":0x1fd8,
                "observed_tcm":[[0,0x7000_0011u32],[0,0]]}});
    }
    assert!(check(&receipt).is_ok());
    for (key,value) in [("core",json!("secondary")),("bank",json!("btcm")),("register",json!(0)),
        ("offset",json!(0x1fd9)),("window_end",json!(0x7000_1000u32)),("physical",json!(0x3000_1fd8u32))] {
        let mut invalid=receipt.clone();
        invalid["contexts"][0]["reads"][1]["provenance"][key]=value;
        assert!(check(&invalid).is_err(),"{key}");
    }
    let mut invalid=receipt.clone();
    invalid["contexts"][0]["reads"][1]["provenance"]["observed_tcm"][0][1]=json!(0x7000_0010u32);
    assert!(check(&invalid).is_err());
    let mut invalid=receipt.clone();
    invalid["contexts"][0]["reads"][0]["provenance"]=receipt["contexts"][0]["reads"][1]["provenance"].clone();
    assert!(check(&invalid).is_err());
    for windows in [
        json!([null,null]),
        json!([{"address":0x7000_0000u32,"bytes":0x2000},null]),
        json!([null,{"address":0x7000_2000u32,"bytes":0x2000}]),
        json!([null,{"address":0x7000_0000u32,"bytes":0x1000}]),
    ] {
        let mut invalid=receipt.clone();
        invalid["contexts"][0]["entry"]["tcm"]=windows;
        assert!(check(&invalid).is_err());
    }
}

#[test]
fn producer_retains_first_payload_refusal_with_irq_execution_and_sampled_depth() {
    use std::{collections::BTreeMap, sync::Arc};
    use fujimod_emulator::{board::{bus::{Bus, Mode}, factory::{self, Graph, Processor, Ram}, memory::BusMapping}, session::Session};
    use fujimod_emulator_cpu::{ExecutionPolicy, Model, Register, Run, arm, timeline::Ratio};
    use fujimod_emulator_cpu_unicorn::rprofile;
    use fujimod_emulator_exr2_plan::Resources;
    use fujimod_xm1::af_ownership::Projection;
    let target=0x0072_503cu32;
    let mut expected=plan();
    expected.sites[0].reads.clear();
    expected.sites[0].register_reads.clear();
    for site in &mut expected.sites { site.word=0xeaff_fffe; }
    let site=expected.sites.iter_mut().find(|site|site.pc==target).unwrap();
    site.register_reads=vec![fujimod_xm1::af_ownership::RegisterRead{register:1,offset:0,bytes:12}];
    let maps=expected_mappings();
    let graph=Graph {
        processors:(0..2).map(|_|Processor {
            model:Model::CortexR5F,mprofile:None,released:true,
            rprofile:Some(rprofile::core::Options{tcm:rprofile::tcm::Options {
                atcm:rprofile::tcm::BankOptions{bytes:0x2000,reset:0x7000_0001},..Default::default()
            },..Default::default()}),
            registers:vec![(Register::Pc,u64::from(target)),(Register::Pstate,0x92),
                (arm::CORE[0],0x1122_3344),(arm::CORE[1],0x7000_1ffc),
                (arm::CORE[13],0x1234_5678),(arm::CORE[14],0x1111_1110)],
        }).collect(),
        ram:vec![Ram{base:0,size:0x0200_0000}],mappings:maps.clone(),
        dma_mappings:vec![BusMapping::ram(0,0x0200_0000)],decoders:vec![],peripherals:vec![],
        instructions_per_tick:Ratio::new(1,1).unwrap(),
    };
    let mut board=graph.build(factory::Options{engine:factory::Engine::Unicorn,translation_cache_bytes:1<<20},Bus::new(Mode::Strict),None).unwrap();
    board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    for register in 2..13 { board.write_register(0,arm::CORE[register],register as u64).unwrap(); }
    for site in &expected.sites { board.write_physical(u64::from(site.pc),&site.word.to_le_bytes()).unwrap(); }
    board.write_physical(0x0005_45a4,&3u32.to_le_bytes()).unwrap();
    let session=Session { definition:Value::Null,board,controls:None,contact_driver:None,resources:Resources::default(),
        nodes:BTreeMap::new(),contacts:BTreeMap::new(),recording_frames:Default::default() };
    use fujimod_emulator::board::observation::{PassiveRetentionStatus as Retention,PassiveStopReason as Stop};
    let capacity_plan=serde_json::from_value(serde_json::to_value(&expected).unwrap()).unwrap();
    let mut projection=Projection::new(expected,&session,Arc::new(maps.clone())).unwrap();
    assert!(serde_json::to_value(&projection).unwrap()["retention"].is_null());
    projection.sync_retention(Retention::Unarmed);
    projection.arm(&session,&maps).unwrap();
    projection.sync_retention(Retention::Armed);
    let mut point=session.board.instruction_points_begin(0,vec![u64::from(target)]).unwrap();
    session.board.cpu(0).unwrap().run(Run{begin:u64::from(target),until:None,instructions:Some(1),watchdog_us:0}).unwrap();
    let entry=point.event_context().unwrap();
    projection.observe(&session,&maps,entry);
    let first=serde_json::to_value(&projection).unwrap();
    assert_eq!(first["status"],json!("blocked"));
    let refusal=first["read_refusal"].clone();
    assert_eq!(refusal["pc"],json!(target));
    assert_eq!(refusal["address"],json!(0x7000_1ffcu32));
    assert_eq!(refusal["bytes"],json!(12));
    assert_eq!(refusal["refusal"]["error"],json!("cross-window"));
    assert_eq!(refusal["execution"]["cpsr"],json!(0x92));
    assert_eq!(refusal["execution"]["registers"],json!([0x1122_3344u32,0x7000_1ffc,2,3,4,5,6,7,8,9,10,11,12]));
    assert_eq!(refusal["execution"]["sp"],json!(0x1234_5678u32));
    assert_eq!(refusal["execution"]["lr"],json!(0x1111_1110u32));
    assert_eq!(refusal["execution"]["sampled_task"]["status"],json!("refused"));
    assert_eq!(refusal["execution"]["sampled_task"]["context_depth"],json!(3));
    assert_eq!(refusal["execution"]["entry"]["address"],json!(target));
    session.board.write_register(0,arm::CORE[1],0xffff_fffc).unwrap();
    projection.observe(&session,&maps,entry);
    projection.sync_retention(Retention::Stopped(Stop::Refused));
    assert_eq!(serde_json::to_value(&projection).unwrap()["retention"],json!({"status":"stopped","reason":"refused"}));
    assert_eq!(serde_json::to_value(&projection).unwrap()["read_refusal"],refusal);
    let typed:fujimod_xm1::af_ownership::ReadRefusal=serde_json::from_value(refusal.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(),refusal);
    session.board.instruction_points_end(0,&mut point).unwrap();
    let arm_plan=serde_json::from_value(serde_json::to_value(&capacity_plan).unwrap()).unwrap();
    let mut capacity=Projection::new(capacity_plan,&session,Arc::new(maps.clone())).unwrap();
    capacity.sync_retention(Retention::Unarmed);
    capacity.arm(&session,&maps).unwrap();
    capacity.sync_retention(Retention::Armed);
    assert!(capacity.active());
    capacity.sync_retention(Retention::Stopped(Stop::CapacityExceeded));
    assert!(!capacity.active());
    let overflow=serde_json::to_value(&capacity).unwrap();
    assert_eq!(overflow["status"],json!("blocked"));
    assert_eq!(overflow["mode"],json!("passive"));
    assert_eq!(overflow["retention"],json!({"status":"stopped","reason":"capacity-exceeded"}));
    assert!(overflow["read_refusal"].is_null());
    capacity.sync_retention(Retention::Stopped(Stop::End));
    assert_eq!(serde_json::to_value(&capacity).unwrap()["retention"],overflow["retention"]);
    let provenance_plan=serde_json::from_value(serde_json::to_value(&arm_plan).unwrap()).unwrap();
    let mut failed_arm=Projection::new(arm_plan,&session,Arc::new(maps.clone())).unwrap();
    failed_arm.sync_retention(Retention::Unarmed);
    assert!(failed_arm.arm(&session,&[]).is_err());
    let failed=serde_json::to_value(&failed_arm).unwrap();
    assert_eq!(failed["status"],json!("blocked"));
    assert!(!failed["failure"].is_null());
    failed_arm.sync_retention(Retention::Stopped(Stop::Refused));
    assert_eq!(serde_json::to_value(&failed_arm).unwrap()["failure"],failed["failure"]);
    let mut provenance=Projection::new(provenance_plan,&session,Arc::new(maps.clone())).unwrap();
    provenance.sync_retention(Retention::Unarmed);
    provenance.arm(&session,&maps).unwrap();
    provenance.sync_retention(Retention::Armed);
    provenance.sync_retention(Retention::Stopped(Stop::ProvenanceUnavailable));
    assert!(!provenance.active());
    let unavailable=serde_json::to_value(&provenance).unwrap();
    assert_eq!(unavailable["status"],json!("blocked"));
    assert_eq!(unavailable["retention"],json!({"status":"stopped","reason":"provenance-unavailable"}));
    assert!(!unavailable["failure"].is_null());
    provenance.sync_retention(Retention::Stopped(Stop::End));
    assert_eq!(serde_json::to_value(&provenance).unwrap()["retention"],unavailable["retention"]);
}

#[test]
fn shared_payload_register_windows_must_match_actual_instruction_entry() {
    let mut receipt=capture();
    for context in receipt["contexts"].as_array_mut().unwrap() {
        context["entry"]["tcm"]=json!([null,{"address":0x7000_0000u32,"bytes":0x2000}]);
        for read in context["reads"].as_array_mut().unwrap() {
            read["provenance"]["observed_tcm"]=json!([[0,0x7000_0011u32],[0,0]]);
        }
    }
    assert!(check(&receipt).is_ok());
    for observed in [
        json!([[0,0],[0,0]]),
        json!([[0x7000_0011u32,0],[0,0]]),
        json!([[0,0x7000_2011u32],[0,0]]),
        json!([[0,0x7000_000du32],[0,0]]),
    ] {
        let mut invalid=receipt.clone();
        for read in invalid["contexts"][0]["reads"].as_array_mut().unwrap() {
            read["provenance"]["observed_tcm"]=observed.clone();
        }
        assert!(check(&invalid).is_err());
    }
}

#[test]
fn refuses_missing_or_forged_passive_retention_completion() {
    for mode in [Value::Null,json!("active"),json!("disabled"),json!({"kind":"passive"})] {
        refuses(|receipt|receipt["mode"]=mode);
    }
    for retention in [
        Value::Null,json!({"status":"disabled"}),json!({"status":"unarmed"}),json!({"status":"armed"}),
        json!({"status":"stopped","reason":"refused"}),json!({"status":"stopped","reason":"capacity-exceeded"}),
        json!({"status":"stopped","reason":"provenance-unavailable"}),
        json!({"status":"unknown"}),json!({"status":"stopped","reason":"unknown"}),
        json!({"status":"stopped"}),json!({"status":"stopped","reason":"end","dropped":0}),
        json!({"status":"armed","reason":"end"}),
    ] {
        refuses(|receipt|receipt["retention"]=retention);
    }
    refuses(|receipt|receipt["schema"]=json!(3));
}

#[test]
fn successful_shared_reads_require_all_four_queried_tcm_registers() {
    for read in 0..2 {
        for core in 0..2 {
            for bank in 0..2 {
                refuses(|receipt|receipt["contexts"][0]["reads"][read]["provenance"]["observed_tcm"][core][bank]=Value::Null);
            }
        }
    }
}

#[test]
fn accepts_a_null_record_pointer_as_a_null_read_row() {
    let mut receipt = capture();
    for context in receipt["contexts"].as_array_mut().unwrap() {
        context["sp"] = json!(4u32);
        context["reads"][1] = Value::Null;
    }
    assert!(check(&receipt).is_ok());
    let mut pointer = capture();
    pointer["contexts"][0]["reads"][1] = Value::Null;
    assert!(check(&pointer).is_err());
    let mut fixed = capture();
    fixed["contexts"][0]["sp"] = json!(4u32);
    fixed["contexts"][0]["reads"][0] = Value::Null;
    assert!(check(&fixed).is_err());
}

#[test]
fn producer_records_a_null_record_pointer_as_a_null_read_row() {
    use std::{collections::BTreeMap, sync::Arc};
    use fujimod_emulator::{board::{bus::{Bus, Mode}, factory::{self, Graph, Processor, Ram}, memory::BusMapping}, session::Session};
    use fujimod_emulator_cpu::{ExecutionPolicy, Model, Register, Run, arm, timeline::Ratio};
    use fujimod_emulator_cpu_unicorn::rprofile;
    use fujimod_emulator_exr2_plan::Resources;
    use fujimod_xm1::af_ownership::Projection;
    let target=0x0072_503cu32;
    let mut expected=plan();
    expected.sites[0].reads.clear();
    expected.sites[0].register_reads.clear();
    for site in &mut expected.sites { site.word=0xeaff_fffe; }
    let site=expected.sites.iter_mut().find(|site|site.pc==target).unwrap();
    site.register_reads=vec![fujimod_xm1::af_ownership::RegisterRead{register:1,offset:0,bytes:76}];
    let maps=expected_mappings();
    let graph=Graph {
        processors:(0..2).map(|_|Processor {
            model:Model::CortexR5F,mprofile:None,released:true,
            rprofile:Some(rprofile::core::Options{tcm:rprofile::tcm::Options {
                atcm:rprofile::tcm::BankOptions{bytes:0x2000,reset:0x7000_0001},..Default::default()
            },..Default::default()}),
            registers:vec![(Register::Pc,u64::from(target)),(Register::Pstate,0x92),
                (arm::CORE[0],0x1122_3344),(arm::CORE[1],0),
                (arm::CORE[13],0x1234_5678),(arm::CORE[14],0x1111_1110)],
        }).collect(),
        ram:vec![Ram{base:0,size:0x0200_0000}],mappings:maps.clone(),
        dma_mappings:vec![BusMapping::ram(0,0x0200_0000)],decoders:vec![],peripherals:vec![],
        instructions_per_tick:Ratio::new(1,1).unwrap(),
    };
    let mut board=graph.build(factory::Options{engine:factory::Engine::Unicorn,translation_cache_bytes:1<<20},Bus::new(Mode::Strict),None).unwrap();
    board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    for register in 2..13 { board.write_register(0,arm::CORE[register],register as u64).unwrap(); }
    for site in &expected.sites { board.write_physical(u64::from(site.pc),&site.word.to_le_bytes()).unwrap(); }
    let session=Session { definition:Value::Null,board,controls:None,contact_driver:None,resources:Resources::default(),
        nodes:BTreeMap::new(),contacts:BTreeMap::new(),recording_frames:Default::default() };
    use fujimod_emulator::board::observation::{PassiveRetentionStatus as Retention};
    let mut projection=Projection::new(expected,&session,Arc::new(maps.clone())).unwrap();
    projection.sync_retention(Retention::Unarmed);
    projection.arm(&session,&maps).unwrap();
    projection.sync_retention(Retention::Armed);
    let mut point=session.board.instruction_points_begin(0,vec![u64::from(target)]).unwrap();
    session.board.cpu(0).unwrap().run(Run{begin:u64::from(target),until:None,instructions:Some(1),watchdog_us:0}).unwrap();
    let entry=point.event_context().unwrap();
    projection.observe(&session,&maps,entry);
    let receipt=serde_json::to_value(&projection).unwrap();
    assert_eq!(receipt["status"],json!("armed"));
    assert!(receipt["read_refusal"].is_null());
    assert_eq!(receipt["contexts"][0]["registers"][1],json!(0));
    assert_eq!(receipt["contexts"][0]["reads"][1],Value::Null);
    session.board.write_register(0,arm::CORE[1],0x7000_1ffc).unwrap();
    projection.observe(&session,&maps,entry);
    let receipt=serde_json::to_value(&projection).unwrap();
    assert_eq!(receipt["status"],json!("blocked"));
    assert_eq!(receipt["read_refusal"]["address"],json!(0x7000_1ffcu32));
    assert_eq!(receipt["read_refusal"]["bytes"],json!(76));
}
