use fujimod_emulator::{
    board::{
        bus::{Bus, Mode},
        factory::{self, Graph, MemoryMap, Processor, Ram},
        memory::BusMapping,
    },
    session::{Session, contact_driver},
};
use fujimod_emulator_cpu::{ExecutionPolicy, Model, Register, arm, timeline::Ratio};
use fujimod_emulator_devices::{
    exr2::{gpio, irq},
    parts::{pins::Pin, switch_panel},
};
use fujimod_emulator_exr2_plan::Resources;
use fujimod_xm1::machine_fingerprint::fingerprint;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use sha2::{Digest, Sha256};
use std::io::Cursor;

const RAM_BYTES: usize = 0x4000;
const GPIO_BASE: u64 = 0xfff8_9000;
const IRQ_BASE: u64 = 0xfffb_3000;
const INPUT_NODE: &str = "gpio";
const BUTTON: &str = "menu";
const BUTTON_PIN: Pin = Pin { port: 0, pin: 0 };

fn session() -> Session {
    session_with_ram(RAM_BYTES)
}

fn session_with_ram(ram_bytes:usize) -> Session {
    fujimod_emulator_exr2_board::register_board_builder();
    fujimod_emulator_models::register();
    let graph = Graph {
        processors: (0..2)
            .map(|core| Processor {
                model: Model::CortexR5F,
                rprofile: Some(Default::default()),
                mprofile: None,
                released: true,
                registers: vec![
                    (Register::Pc, 0x1000 + core as u64 * 0x1000),
                    (Register::Pstate, 0xd3),
                    (arm::CORE[0], 0),
                ],
            })
            .collect(),
        ram: vec![Ram { base: 0, size: ram_bytes }],
        dma_mappings: vec![BusMapping::ram(0, ram_bytes)],
        mappings: (0..2)
            .map(|core| MemoryMap {
                core,
                address: 0,
                physical: 0,
                size: ram_bytes,
                permissions: 7,
            })
            .collect(),
        decoders: vec![],
        peripherals: vec![],
        instructions_per_tick: Ratio::new(24, 1).unwrap(),
    };
    let mut bus = Bus::new(Mode::Strict);
    let gpio = gpio::Gpio::new(
        gpio::Options { input_levels: [0; 16], output_levels: Vec::new() },
        gpio::Wiring { irqs: Vec::new() },
    ).unwrap();
    let pads = gpio.pads();
    let gpio_id = bus.register(GPIO_BASE, 0x480, Box::new(gpio)).unwrap();
    let irq_inputs = irq::Inputs::default();
    let controller = irq::InterruptController::new(
        irq::Options { arbitration: irq::Arbitration::Unique },
        irq::Wiring {
            base: IRQ_BASE,
            inputs: irq_inputs.clone(),
            enables: None,
            software: vec![],
        },
    ).unwrap();
    let irq_id = bus.register(IRQ_BASE, 0x1000, Box::new(controller)).unwrap();
    let mut board = graph.build(
        factory::Options {
            engine: factory::Engine::Unicorn,
            translation_cache_bytes: 1 << 20,
        },
        bus,
        None,
    ).unwrap();
    board.set_execution_policy(ExecutionPolicy::Exact).unwrap();
    board.configure_core_interrupts(0, irq_id, Box::new(irq_inputs)).unwrap();
    let mut resources = Resources::default();
    resources.gpio.insert(INPUT_NODE.into(), pads);
    let mut session = Session {
        definition: Value::Null,
        board,
        controls: None,
        contact_driver: None,
        resources,
        nodes: BTreeMap::from([(INPUT_NODE.into(), gpio_id)]),
        contacts: BTreeMap::new(),
        recording_frames: Default::default(),
    };
    session.configure_controls(switch_panel::Spec {
        controls: vec![switch_panel::Control {
            id: BUTTON.into(),
            label: "MENU/OK".into(),
            kind: "button".into(),
            positions: vec!["released".into(), "pressed".into()],
            initial: "released".into(),
            unavailable: None,
        }],
        circuits: vec![switch_panel::Circuit {
            controls: vec![BUTTON.into()],
            cases: [false, true].into_iter().map(|high| switch_panel::Case {
                positions: vec![if high { "pressed" } else { "released" }.into()],
                signals: vec![switch_panel::Signal::Gpio {
                    node: INPUT_NODE.into(),
                    pin: BUTTON_PIN,
                    high,
                }],
            }).collect(),
        }],
    }).unwrap();
    session.configure_contact_driver(contact_driver::Spec {
        poll_ticks: 1,
        max_phase_ticks: 10,
        actuators: vec![contact_driver::Actuator {
            id: BUTTON.into(),
            momentary: true,
            states: ["released", "pressed"].into_iter().map(|position| {
                let check = contact_driver::Check { address: 0, bytes: vec![1], pointer: None };
                contact_driver::Pose {
                    position: position.into(),
                    ready: vec![check.clone()],
                    expected: vec![check],
                }
            }).collect(),
        }],
        interfaces: vec![],
        catalog: vec![contact_driver::ControlInput {
            id: BUTTON.into(),
            label: "MENU/OK".into(),
            kind: "button".into(),
            supported: true,
            unavailable: None,
            positions: None,
            initial: None,
            relative: None,
            width: None,
            height: None,
            stages: None,
        }],
    }).unwrap();
    session
}

fn proof(session: &Session) -> Value {
    let counts = session.board.instruction_counts();
    let now = session.board.now();
    let proof = fingerprint(session).expect("complete stopped-machine fingerprint");
    assert_eq!(proof["schema"], json!(1));
    assert_eq!(proof["status"], json!("complete"));
    assert_eq!(proof["counts"], json!(counts));
    assert_eq!(proof["now"], json!(now));
    assert_eq!(proof["included_state"], json!("session-v2/board-v1-complete-machine"));
    assert_eq!(proof["excluded_host_fields"], json!([
        "board.entries", "board.hooks", "board.observations",
    ]));
    assert!(proof["logical_bytes"].as_u64().is_some_and(|bytes| bytes > 0));
    let digest = proof["machine_sha256"].as_str().expect("SHA-256 string");
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let components=&proof["components"];
    assert_eq!(components["schema"],1);
    assert_eq!(components["algorithm"],"sha256-hierarchical-v1");
    assert_eq!(components["chunk_bytes"],1048576);
    let entries=components["entries"].as_array().unwrap();
    assert_eq!(entries[0]["path"],"$");
    assert_eq!(entries[0]["kind"],"checkpoint");
    assert_eq!(entries[0]["sha256"],components["root_sha256"]);
    assert!(serde_json::to_vec(components).unwrap().len()<=16*1024*1024);
    for entry in entries {
        assert!(entry["logical_bytes"].is_u64());
        assert_eq!(entry["sha256"].as_str().unwrap().len(),64);
        assert!(entry.get("bytes").is_none());
    }
    assert_eq!(session.board.instruction_counts(), counts);
    assert_eq!(session.board.now(), now);
    proof
}

#[test]
fn repeated_fingerprints_preserve_the_stopped_machine() {
    let session = session();
    let before = session.fingerprint().unwrap();
    let initial = proof(&session);
    assert_eq!(initial["counts"], json!([0, 0]));
    assert_eq!(initial["now"], json!(0));
    assert_eq!(proof(&session), initial);
    assert_eq!(session.fingerprint().unwrap(), before);
}

#[derive(Clone, Copy, Debug)]
enum Mutation {
    PrimaryRegister,
    PeerRegister,
    Ram,
    DeviceInput,
    QueuedInput,
    Timeline,
    Irq,
    Scheduler,
}

#[test]
fn every_machine_state_control_changes_the_digest_and_restores() {
    let mut session = session();
    let saved = session.checkpoint(None).unwrap();
    let initial = proof(&session);
    for mutation in [
        Mutation::PrimaryRegister,
        Mutation::PeerRegister,
        Mutation::Ram,
        Mutation::DeviceInput,
        Mutation::QueuedInput,
        Mutation::Timeline,
        Mutation::Irq,
        Mutation::Scheduler,
    ] {
        match mutation {
            Mutation::PrimaryRegister => session.board.write_register(0, arm::CORE[0], 1).unwrap(),
            Mutation::PeerRegister => session.board.write_register(1, arm::CORE[0], 1).unwrap(),
            Mutation::Ram => session.board.write_physical(0x3000, &[1]).unwrap(),
            Mutation::DeviceInput => {
                session.resources.pads(INPUT_NODE).unwrap().request_many(&[(BUTTON_PIN, true)]).unwrap();
                session.board.inputs_changed(session.node(INPUT_NODE).unwrap()).unwrap();
            }
            Mutation::QueuedInput => {
                session.contact_input(BUTTON, None, false).unwrap();
                let controls = session.control_state().unwrap();
                assert_eq!(controls.positions[BUTTON], "released");
                let queue = controls.input_queue.unwrap();
                assert!(queue.pending > 0 || queue.active.is_some());
            }
            Mutation::Timeline => {
                session.board.harness_advance(1, 1).unwrap();
                assert_eq!(session.board.now(), 1);
            }
            Mutation::Irq => session.board.set_core_irq(0, 32, true).unwrap(),
            Mutation::Scheduler => session.board.set_execution_policy(ExecutionPolicy::Interactive).unwrap(),
        }
        let changed = proof(&session);
        assert_ne!(changed["machine_sha256"], initial["machine_sha256"], "{mutation:?}");
        assert_ne!(changed["components"]["root_sha256"],initial["components"]["root_sha256"],"{mutation:?}");
        let before=initial["components"]["entries"].as_array().unwrap();
        let after=changed["components"]["entries"].as_array().unwrap();
        assert!(after.iter().any(|entry| entry["kind"]!="checkpoint" && !before.contains(entry)),"{mutation:?}");
        assert_eq!(changed["counts"], json!([0, 0]), "{mutation:?}");
        session.restore(&saved).unwrap();
        assert_eq!(proof(&session), initial, "{mutation:?}");
    }
}

#[test]
fn observer_point_membership_does_not_change_machine_identity() {
    let session = session();
    let initial = proof(&session);
    let mut primary = session.board.instruction_points_begin_mutable(0, vec![0x1000]).unwrap();
    let mut peer = session.board.instruction_points_begin(1, vec![0x2000]).unwrap();
    assert_eq!(proof(&session), initial);
    primary.replace_addresses(vec![0x1000, 0x1004, 0x1010]).unwrap();
    assert_eq!(proof(&session), initial);
    primary.replace_addresses(Vec::new()).unwrap();
    assert_eq!(proof(&session), initial);
    session.board.instruction_points_end(1, &mut peer).unwrap();
    session.board.instruction_points_end(0, &mut primary).unwrap();
    assert_eq!(proof(&session), initial);
}

#[test]
fn bus_components_cover_serialized_state_and_device_children() {
    let session=session();
    let initial=proof(&session);
    let entries=initial["components"]["entries"].as_array().unwrap();
    let bus_path="$/children/0/children/1";
    assert!(entries.iter().any(|entry|entry["path"]==bus_path && entry["checkpoint_kind"]=="board-bus-v1"));
    assert!(entries.iter().any(|entry|entry["path"]==format!("{bus_path}/children/0") && entry["kind"]=="checkpoint"));
    assert!(entries.iter().any(|entry|entry["path"]==format!("{bus_path}/children/1") && entry["kind"]=="checkpoint"));
    let header_path=format!("{bus_path}/header");
    let before=entries.iter().find(|entry|entry["path"]==header_path).unwrap();
    session.board.observe_transactions(Some(10));
    let changed=proof(&session);
    let after=changed["components"]["entries"].as_array().unwrap().iter()
        .find(|entry|entry["path"]==header_path).unwrap();
    assert_ne!(before["sha256"],after["sha256"]);
    assert_ne!(initial["machine_sha256"],changed["machine_sha256"]);
    assert_ne!(initial["components"]["root_sha256"],changed["components"]["root_sha256"]);
}

fn original_canonical_bytes(point:&Value,is_board:bool,output:&mut Vec<u8>) {
    fn number(output:&mut Vec<u8>,value:usize) {output.extend_from_slice(&(value as u64).to_le_bytes());}
    fn field(output:&mut Vec<u8>,value:&[u8]) {number(output,value.len());output.extend_from_slice(value);}
    let kind=point["kind"].as_str().unwrap();
    let mut bytes:Vec<u8>=point["bytes"].as_array().unwrap().iter().map(|byte|byte.as_u64().unwrap() as u8).collect();
    if is_board {
        let mut cursor=Cursor::new(bytes.as_slice());
        let ciborium_ll::Header::Map(Some(count))=ciborium_ll::Decoder::from(&mut cursor).pull().unwrap() else {panic!("board map")};
        let mut retained=Vec::new();
        for _ in 0..count {
            let start=cursor.position() as usize;
            let name:String=ciborium::from_reader(&mut cursor).unwrap();
            let _:serde::de::IgnoredAny=ciborium::from_reader(&mut cursor).unwrap();
            if !["entries","hooks","observations"].contains(&name.as_str()) {
                retained.extend_from_slice(&bytes[start..cursor.position() as usize]);
            }
        }
        let mut normalized=Vec::new();
        ciborium_ll::Encoder::from(&mut normalized).push(ciborium_ll::Header::Map(Some(count-3))).unwrap();
        normalized.extend_from_slice(&retained);
        bytes=normalized;
    }
    field(output,kind.as_bytes());
    field(output,&bytes);
    let blobs=point["blobs"].as_array().unwrap();
    number(output,blobs.len());
    for blob in blobs {
        let bytes:Vec<u8>=blob.as_array().unwrap().iter().map(|byte|byte.as_u64().unwrap() as u8).collect();
        field(output,&bytes);
    }
    let children=point["children"].as_array().unwrap();
    number(output,children.len());
    for (index,child) in children.iter().enumerate() {
        original_canonical_bytes(child,kind=="emulator-session-v2" && index==0,output);
    }
}

#[test]
fn component_extension_preserves_the_original_whole_machine_framing() {
    let session=session_with_ram(1048576+4096);
    session.board.write_physical(1048576+17,b"abc").unwrap();
    let checkpoint=session.checkpoint(None).unwrap().export_checkpoint().unwrap();
    let serialized=serde_json::to_value(checkpoint).unwrap();
    let domain=b"af-machine-fingerprint-v1";
    let mut canonical=Vec::new();
    canonical.extend_from_slice(&(domain.len() as u64).to_le_bytes());
    canonical.extend_from_slice(domain);
    original_canonical_bytes(&serialized,false,&mut canonical);
    let actual=proof(&session);
    assert_eq!(actual["logical_bytes"],canonical.len());
    assert_eq!(actual["machine_sha256"],format!("{:x}",Sha256::digest(&canonical)));
    let entries=actual["components"]["entries"].as_array().unwrap();
    let mut input=&canonical[8+domain.len()..];
    let expected=authenticate_components(&mut input,"$",entries);
    assert!(input.is_empty());
    assert_eq!(actual["components"]["root_sha256"],format!("{expected:x}"));
    eprintln!("fingerprint canonical_bytes={} component_metadata_bytes={} component_entries={}",
        canonical.len(),serde_json::to_vec(&actual["components"]).unwrap().len(),entries.len());
}

#[test]
fn ram_write_localizes_the_data_chunk_without_execution() {
    let session=session_with_ram(3*1024*1024);
    session.board.write_physical(0,&vec![0x31;3*1024*1024]).unwrap();
    let mut physical_byte=[0];
    session.board.read_physical(1024*1024+17,&mut physical_byte).unwrap();
    assert_eq!(physical_byte,[0x31]);
    let initial=proof(&session);
    let original_blob=authenticated_ram_blob(&session,&initial);
    assert!(original_blob.len()>2*1024*1024);
    session.board.write_physical(1024*1024+17,&[0x5a]).unwrap();
    session.board.read_physical(1024*1024+17,&mut physical_byte).unwrap();
    assert_eq!(physical_byte,[0x5a]);
    let changed=proof(&session);
    let changed_blob=authenticated_ram_blob(&session,&changed);
    assert_eq!(original_blob.len(),changed_blob.len());
    let differences:Vec<_>=original_blob.iter().zip(&changed_blob).enumerate()
        .filter(|(_, (before,after))|before!=after).map(|(index,_)|index).collect();
    assert_eq!(differences.len(),1);
    let serialized_offset=differences[0];
    assert_eq!(original_blob[serialized_offset],0x31);
    assert_eq!(changed_blob[serialized_offset],0x5a);
    let before=initial["components"]["entries"].as_array().unwrap();
    let after=changed["components"]["entries"].as_array().unwrap();
    let ram_chunks:Vec<_>=after.iter().filter(|entry|entry["checkpoint_kind"]=="board-memory-v1" && entry["kind"]=="chunk").collect();
    assert_eq!(ram_chunks.len(),changed_blob.len().div_ceil(1024*1024));
    let changed_chunk_offset=serialized_offset/(1024*1024)*(1024*1024);
    let changed_chunk=ram_chunks.iter().find(|entry|entry["offset"]==changed_chunk_offset).unwrap();
    for (index,(original_bytes,current_bytes)) in original_blob.chunks(1024*1024).zip(changed_blob.chunks(1024*1024)).enumerate() {
        let offset=index*1024*1024;
        let chunk=ram_chunks.iter().find(|entry|entry["offset"]==offset).unwrap();
        let path=chunk["path"].as_str().unwrap();
        let original=before.iter().find(|entry|entry["path"]==path).unwrap();
        assert_eq!(original["sha256"],format!("{:x}",Sha256::digest(original_bytes)));
        assert_eq!(chunk["sha256"],format!("{:x}",Sha256::digest(current_bytes)));
        assert_eq!(chunk["logical_bytes"],current_bytes.len());
        if offset==changed_chunk_offset {
            assert_ne!(original["sha256"],chunk["sha256"]);
        } else {
            assert_eq!(original,*chunk,"{path}");
        }
    }
    let changed_path=changed_chunk["path"].as_str().unwrap();
    for ancestor in after.iter().filter(|entry| {
        let path=entry["path"].as_str().unwrap();
        changed_path.strip_prefix(path).is_some_and(|suffix|suffix.starts_with('/'))
    }) {
        let original=before.iter().find(|entry|entry["path"]==ancestor["path"]).unwrap();
        assert_ne!(original["sha256"],ancestor["sha256"]);
    }
    assert_ne!(initial["machine_sha256"],changed["machine_sha256"]);
    assert_ne!(initial["components"]["root_sha256"],changed["components"]["root_sha256"]);
    assert_eq!(changed["counts"],json!([0,0]));
    assert_eq!(changed["now"],0);
}

fn authenticated_ram_blob(session:&Session,proof:&Value)->Vec<u8> {
    fn memory(point:&Value)->Option<&Value> {
        if point["kind"]=="board-memory-v1" {return Some(point);}
        point["children"].as_array().unwrap().iter().find_map(memory)
    }
    let checkpoint=session.checkpoint(None).unwrap().export_checkpoint().unwrap();
    let serialized=serde_json::to_value(checkpoint).unwrap();
    let mut canonical=Vec::new();
    original_canonical_bytes(&serialized,false,&mut canonical);
    let mut input=canonical.as_slice();
    let expected=authenticate_components(&mut input,"$",proof["components"]["entries"].as_array().unwrap());
    assert!(input.is_empty());
    assert_eq!(proof["components"]["root_sha256"],format!("{expected:x}"));
    let blobs=memory(&serialized).unwrap()["blobs"].as_array().unwrap();
    assert_eq!(blobs.len(),1);
    blobs[0].as_array().unwrap().iter().map(|byte|byte.as_u64().unwrap() as u8).collect()
}

fn authenticate_components(input:&mut &[u8],path:&str,entries:&[Value])->sha2::digest::Output<Sha256> {
    fn take_number(input:&mut &[u8])->usize {
        let (number,rest)=input.split_at(8);
        *input=rest;
        u64::from_le_bytes(number.try_into().unwrap()).try_into().unwrap()
    }
    fn take_field<'a>(input:&mut &'a [u8])->&'a [u8] {
        let len=take_number(input);
        let (field,rest)=input.split_at(len);
        *input=rest;
        field
    }
    fn framed(bytes:&[u8])->Vec<u8> {
        let mut frame=(bytes.len() as u64).to_le_bytes().to_vec();
        frame.extend_from_slice(bytes);
        frame
    }
    fn assert_entry(entries:&[Value],path:&str,kind:&str,length:usize,expected:&sha2::digest::Output<Sha256>) {
        let matches:Vec<_>=entries.iter().filter(|entry|entry["path"]==path).collect();
        assert_eq!(matches.len(),1,"{path}");
        let entry=matches[0];
        assert_eq!(entry["kind"],kind,"{path}");
        assert_eq!(entry["logical_bytes"],length,"{path}");
        assert_eq!(entry["sha256"],format!("{expected:x}"),"{path}");
    }
    let start=input.len();
    let kind=take_field(input);
    let header=take_field(input);
    let header_digest=Sha256::digest(header);
    assert_entry(entries,&format!("{path}/header"),"header",header.len(),&header_digest);
    let mut node=framed(b"af-machine-components-v1");
    node.extend(framed(b"checkpoint"));
    node.extend(framed(kind));
    node.extend_from_slice(&(header.len() as u64).to_le_bytes());
    node.extend_from_slice(&header_digest);
    let blobs=take_number(input);
    node.extend_from_slice(&(blobs as u64).to_le_bytes());
    for index in 0..blobs {
        let blob=take_field(input);
        let blob_path=format!("{path}/blobs/{index}");
        let chunks:Vec<_>=blob.chunks(1048576).collect();
        let mut blob_preimage=framed(b"af-machine-components-v1");
        blob_preimage.extend(framed(b"blob"));
        blob_preimage.extend_from_slice(&(blob.len() as u64).to_le_bytes());
        blob_preimage.extend_from_slice(&(chunks.len() as u64).to_le_bytes());
        for (index,chunk) in chunks.into_iter().enumerate() {
            let digest=Sha256::digest(chunk);
            let chunk_path=format!("{blob_path}/chunks/{index}");
            assert_entry(entries,&chunk_path,"chunk",chunk.len(),&digest);
            let entry=entries.iter().find(|entry|entry["path"]==chunk_path).unwrap();
            assert_eq!(entry["offset"],index*1048576);
            blob_preimage.extend_from_slice(&(chunk.len() as u64).to_le_bytes());
            blob_preimage.extend_from_slice(&digest);
        }
        let digest=Sha256::digest(&blob_preimage);
        assert_entry(entries,&blob_path,"blob",blob.len(),&digest);
        node.extend_from_slice(&(blob.len() as u64).to_le_bytes());
        node.extend_from_slice(&digest);
    }
    let children=take_number(input);
    node.extend_from_slice(&(children as u64).to_le_bytes());
    for index in 0..children {
        let before=input.len();
        let digest=authenticate_components(input,&format!("{path}/children/{index}"),entries);
        node.extend_from_slice(&((before-input.len()) as u64).to_le_bytes());
        node.extend_from_slice(&digest);
    }
    let digest=Sha256::digest(&node);
    assert_entry(entries,path,"checkpoint",start-input.len(),&digest);
    digest
}

#[test]
fn exact_archive_preserves_original_bytes_and_independent_cbor_fields() {
    use fujimod_xm1::exact_cpu_archive::{self, Architecture};
    let session = session();
    let receipt = proof(&session);
    let decoded = exact_cpu_archive::authenticate(&receipt).unwrap();
    let checkpoint = session.checkpoint(None).unwrap().export_checkpoint().unwrap();
    for (index, cpu) in decoded.cpu.iter().enumerate() {
        let blob = &receipt["exact_state"]["cpu"][index];
        let raw = exact_cpu_archive::unhex(blob["bytes_hex"].as_str().unwrap()).unwrap();
        assert_eq!(raw.as_slice(), checkpoint.children()[0].blobs()[index].as_ref());
        assert_eq!(blob["sha256"], format!("{:x}", Sha256::digest(&raw)));
        let independently_decoded: ciborium::Value = ciborium::from_reader(raw.as_slice()).unwrap();
        let mut reencoded = Vec::new();
        ciborium::into_writer(&cpu.state, &mut reencoded).unwrap();
        let typed_fields: ciborium::Value = ciborium::from_reader(reencoded.as_slice()).unwrap();
        assert_eq!(typed_fields, independently_decoded);
        assert_eq!(reencoded, raw);
        let mut architectural = Vec::new();
        ciborium::into_writer(&cpu.architecture, &mut architectural).unwrap();
        assert_eq!(architectural, cpu.state.context);
        let Architecture::CortexR5f(registers) = &cpu.architecture.architecture;
        assert_eq!(registers.registers[15] as u64, session.board.read_register(index, Register::Pc).unwrap());
        assert_eq!(registers.cpsr as u64, session.board.read_register(index, Register::Pstate).unwrap());
    }
    let exclusive = &receipt["exact_state"]["exclusive"];
    let raw = exact_cpu_archive::unhex(exclusive["bytes_hex"].as_str().unwrap()).unwrap();
    assert_eq!(raw.as_slice(), checkpoint.children()[0].blobs()[2].as_ref());
    assert_eq!(decoded.exclusive.epoch, u128::from_le_bytes(raw[8..24].try_into().unwrap()));
    assert!(serde_json::to_vec(&receipt).unwrap().len() <= 16 * 1024 * 1024);
}

#[test]
fn exact_archive_rejects_payload_digest_path_capture_and_component_tampering() {
    use fujimod_xm1::exact_cpu_archive::authenticate;
    let original = proof(&session());
    for field in ["bytes_hex", "sha256", "component_sha256", "path", "format", "length"] {
        let mut changed = original.clone();
        changed["exact_state"]["cpu"][0][field] = if field == "length" { json!(1) } else { json!("00") };
        assert!(authenticate(&changed).is_err(), "{field}");
    }
    for field in ["counts", "now", "checkpoint_model_id", "model_definition_sha256", "executable_sha256"] {
        let mut changed = original.clone();
        changed["exact_state"][field] = match field { "counts" => json!([1, 0]), "now" => json!(1), _ => json!("bad") };
        assert!(authenticate(&changed).is_err(), "{field}");
    }
    let mut changed = original.clone();
    let entries = changed["components"]["entries"].as_array_mut().unwrap();
    let entry = entries.iter_mut().find(|entry| entry["path"] == "$/children/0/blobs/0").unwrap();
    entry["sha256"] = json!("00");
    assert!(authenticate(&changed).is_err());
}

#[test]
fn exact_cpu_rejects_unknown_missing_duplicate_and_trailing_fields() {
    use fujimod_xm1::exact_cpu_archive::{decode_cpu, unhex};
    let receipt = proof(&session());
    let raw = unhex(receipt["exact_state"]["cpu"][0]["bytes_hex"].as_str().unwrap()).unwrap();
    let value: ciborium::Value = ciborium::from_reader(raw.as_slice()).unwrap();
    let ciborium::Value::Map(fields) = value else { panic!("CPU map") };
    for index in 0..fields.len() {
        let mut missing = fields.clone();
        missing.remove(index);
        let mut bytes = Vec::new();
        ciborium::into_writer(&ciborium::Value::Map(missing), &mut bytes).unwrap();
        assert!(decode_cpu(&bytes).is_err(), "missing {:?}", fields[index].0);
    }
    for pair in [
        (ciborium::Value::Text("unknown".into()), ciborium::Value::Null),
        fields[0].clone(),
    ] {
        let mut changed = fields.clone();
        changed.push(pair);
        let mut bytes = Vec::new();
        ciborium::into_writer(&ciborium::Value::Map(changed), &mut bytes).unwrap();
        assert!(decode_cpu(&bytes).is_err());
    }
    let mut trailing = raw.clone();
    trailing.push(0);
    assert!(decode_cpu(&trailing).is_err());
    for end in [0, 7, raw.len() - 1] { assert!(decode_cpu(&raw[..end]).is_err()); }
}

#[test]
fn capture_and_observer_membership_preserve_nonempty_reservations_and_strex_behavior() {
    use fujimod_emulator_cpu::Run;
    use fujimod_xm1::exact_cpu_archive::authenticate;
    let mut outcomes = Vec::new();
    for capture in [false, true] {
        for observer in [false, true] {
            let session = session();
            let program: Vec<u8> = [0xe192_1f9fu32, 0xe281_1001, 0xe182_3f91].into_iter().flat_map(u32::to_le_bytes).collect();
            for core in 0..2 {
                session.board.write_physical(0x1000 + core as u64 * 0x1000, &program).unwrap();
                session.board.write_register(core, arm::CORE[2], 0x3000 + core as u64 * 0x100).unwrap();
            }
            let mut points = observer.then(|| session.board.instruction_points_begin(0, vec![0x1000, 0x1004]).unwrap());
            let run = |core: usize, count: usize| {
                let cpu = session.board.cpu(core).unwrap();
                let mut retired = 0;
                for _ in 0..4 {
                    if retired == count { break; }
                    let pc = session.board.read_register(core, Register::Pc).unwrap();
                    cpu.run(Run { begin: pc, until: None, instructions: Some(count - retired), watchdog_us: 0 }).unwrap();
                    let progress = usize::try_from(cpu.instruction_progress().unwrap()).unwrap();
                    assert!(progress <= count - retired);
                    retired += progress;
                    if core == 0 {
                        if let Some(points) = points.as_ref() {
                            if let Some(observed) = points.event() {
                                points.resume(session.board.read_register(core, Register::Pc).unwrap(), observed);
                            }
                        }
                    }
                }
                assert_eq!(retired, count);
            };
            for core in 0..2 {
                run(core, 1);
                assert_eq!(session.board.read_register(core, Register::Pc).unwrap(), 0x1004 + core as u64 * 0x1000);
            }
            if capture {
                let receipt = fingerprint(&session).unwrap();
                let state = authenticate(&receipt).unwrap();
                assert!(state.cpu.iter().all(|cpu| cpu.reservation.reservation.is_some()));
                assert!(state.exclusive.slots.iter().all(|slot| slot.reservation.is_some()));
                assert_eq!(fingerprint(&session).unwrap(), receipt);
            }
            for core in 0..2 {
                run(core, 2);
                assert_eq!(session.board.read_register(core, Register::Pc).unwrap(), 0x100c + core as u64 * 0x1000);
                assert_eq!(session.board.read_register(core, arm::CORE[3]).unwrap(), 0);
            }
            let mut data = [0; 8];
            session.board.read_physical(0x3000, &mut data[..4]).unwrap();
            session.board.read_physical(0x3100, &mut data[4..]).unwrap();
            assert_eq!(u32::from_le_bytes(data[0..4].try_into().unwrap()), 1);
            assert_eq!(u32::from_le_bytes(data[4..8].try_into().unwrap()), 1);
            outcomes.push(data);
            if let Some(points) = points.as_mut() { session.board.instruction_points_end(0, points).unwrap(); }
        }
    }
    assert!(outcomes.iter().all(|data| *data == outcomes[0]));
}

#[test]
fn unchanged_memory_write_retains_epoch_change_and_invalidates_exclusive_store() {
    use fujimod_emulator_cpu::Run;
    use fujimod_xm1::exact_cpu_archive::authenticate;
    let mut session = session();
    let program: Vec<u8> = [0xe192_1f9fu32, 0xe182_3f91].into_iter().flat_map(u32::to_le_bytes).collect();
    session.board.write_physical(0x1000, &program).unwrap();
    session.board.write_register(0, arm::CORE[2], 0x3000).unwrap();
    let run = |session: &Session, pc| {
        let cpu = session.board.cpu(0).unwrap();
        cpu.run(Run { begin: pc, until: None, instructions: Some(1), watchdog_us: 0 }).unwrap();
        assert_eq!(cpu.instruction_progress(), Some(1));
        assert_eq!(session.board.read_register(0, Register::Pc).unwrap(), pc + 4);
    };
    run(&session, 0x1000);
    let initial = proof(&session);
    let before = authenticate(&initial).unwrap();
    assert!(before.cpu[0].reservation.reservation.is_some());
    assert!(before.exclusive.slots[0].reservation.is_some());
    let saved = session.checkpoint(None).unwrap();
    assert_eq!(proof(&session), initial);
    run(&session, 0x1004);
    assert_eq!(session.board.read_register(0, arm::CORE[3]).unwrap(), 0);
    session.restore(&saved).unwrap();
    assert_eq!(proof(&session), initial);
    session.board.write_physical(0x3000, &[0; 4]).unwrap();
    let changed = proof(&session);
    let after = authenticate(&changed).unwrap();
    assert!(after.exclusive.epoch > before.exclusive.epoch);
    assert!(after.cpu[0].reservation.reservation.is_none());
    assert!(after.exclusive.slots[0].reservation.is_none());
    for core in 0..2 {
        assert!(after.cpu[core].reservation.epoch > before.cpu[core].reservation.epoch);
        assert_eq!(after.cpu[core].architecture, before.cpu[core].architecture);
        assert_eq!(after.cpu[core].tcm, before.cpu[core].tcm);
    }
    let memory_entries = |receipt: &Value| receipt["components"]["entries"].as_array().unwrap().iter()
        .filter(|entry| entry["checkpoint_kind"] == "board-memory-v1").cloned().collect::<Vec<_>>();
    assert_eq!(memory_entries(&initial), memory_entries(&changed));
    assert_ne!(initial["machine_sha256"], changed["machine_sha256"]);
    assert_ne!(initial["exact_state"]["exclusive"]["bytes_hex"], changed["exact_state"]["exclusive"]["bytes_hex"]);
    run(&session, 0x1004);
    assert_eq!(session.board.read_register(0, arm::CORE[3]).unwrap(), 1);
    let mut bytes = [0; 4];
    session.board.read_physical(0x3000, &mut bytes).unwrap();
    assert_eq!(bytes, [0; 4]);
}

#[test]
fn well_formed_but_wrong_model_and_executable_bindings_are_rejected() {
    use fujimod_xm1::exact_cpu_archive::{authenticate_bindings, unhex, CheckpointModelId, ImmutableBindings, Sha256Identity};
    let receipt = proof(&session());
    let binding = |field: &str| Sha256Identity(unhex(receipt["exact_state"][field].as_str().unwrap()).unwrap().try_into().unwrap());
    let expected = ImmutableBindings {
        checkpoint_model_id: CheckpointModelId(env!("XM1_CHECKPOINT_MODEL_ID").into()),
        model_definition: binding("model_definition_sha256"), executable: binding("executable_sha256"),
    };
    authenticate_bindings(&receipt, &expected).unwrap();
    for field in ["model_definition_sha256", "executable_sha256"] {
        let mut changed = receipt.clone();
        let mut bytes = unhex(changed["exact_state"][field].as_str().unwrap()).unwrap();
        bytes[0] ^= 1;
        changed["exact_state"][field] = json!(fujimod_xm1::exact_cpu_archive::hex(&bytes));
        assert!(authenticate_bindings(&changed, &expected).is_err(), "{field}");
    }
    let wrong_model = ImmutableBindings { checkpoint_model_id: CheckpointModelId("0".repeat(64)), ..expected };
    assert!(authenticate_bindings(&receipt, &wrong_model).is_err());
}

#[test]
fn coordinated_exact_blob_tampering_cannot_leave_stale_chunks_or_ancestors() {
    use fujimod_xm1::exact_cpu_archive::{self, Architecture};
    let original = proof(&session());
    let decoded = exact_cpu_archive::authenticate(&original).unwrap();
    let mut cpu = decoded.cpu[0].state.clone();
    let mut architecture = decoded.cpu[0].architecture.clone();
    let Architecture::CortexR5f(registers) = &mut architecture.architecture;
    registers.registers[0] = 1;
    cpu.context.clear();
    ciborium::into_writer(&architecture, &mut cpu.context).unwrap();
    let mut raw = Vec::new();
    ciborium::into_writer(&cpu, &mut raw).unwrap();
    assert_eq!(raw.len(), original["exact_state"]["cpu"][0]["length"].as_u64().unwrap() as usize);
    exact_cpu_archive::decode_cpu(&raw).unwrap();
    let mut changed = original.clone();
    let blob_digest = exact_cpu_archive::blob_component_digest(&raw);
    changed["exact_state"]["cpu"][0]["bytes_hex"] = json!(exact_cpu_archive::hex(&raw));
    changed["exact_state"]["cpu"][0]["sha256"] = json!(format!("{:x}", Sha256::digest(&raw)));
    changed["exact_state"]["cpu"][0]["component_sha256"] = json!(blob_digest);
    let entries = changed["components"]["entries"].as_array_mut().unwrap();
    entries.iter_mut().find(|entry| entry["path"] == "$/children/0/blobs/0").unwrap()["sha256"] = json!(blob_digest);
    assert!(exact_cpu_archive::authenticate(&changed).is_err());
    for (index, chunk) in raw.chunks(1048576).enumerate() {
        let path = format!("$/children/0/blobs/0/chunks/{index}");
        let entry = changed["components"]["entries"].as_array_mut().unwrap().iter_mut().find(|entry| entry["path"] == path).unwrap();
        entry["sha256"] = json!(format!("{:x}", Sha256::digest(chunk)));
    }
    assert!(exact_cpu_archive::authenticate(&changed).is_err());
    for path in ["$/header", "$/children/0/header", "$/children/0", "$"] {
        let mut changed = original.clone();
        let entry = changed["components"]["entries"].as_array_mut().unwrap().iter_mut().find(|entry| entry["path"] == path).unwrap();
        entry["sha256"] = json!("0".repeat(64));
        assert!(exact_cpu_archive::authenticate(&changed).is_err(), "{path}");
    }
    let mut unknown = original.clone();
    unknown["components"]["entries"][0]["unrecognized"] = json!(0);
    assert!(exact_cpu_archive::authenticate(&unknown).is_err());
    let mut missing = original.clone();
    missing["components"]["entries"].as_array_mut().unwrap().pop();
    assert!(exact_cpu_archive::authenticate(&missing).is_err());
    let mut duplicate = original.clone();
    let extra = duplicate["components"]["entries"][0].clone();
    duplicate["components"]["entries"].as_array_mut().unwrap().push(extra);
    assert!(exact_cpu_archive::authenticate(&duplicate).is_err());
}
