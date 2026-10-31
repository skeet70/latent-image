use fujimod_xm1::peer_witness::PeerContract;

#[test]
fn accepts_bounded_pre_execution_observation() {
    assert!(PeerContract::parse(br#"{"schema":1,"budget":20000000}"#).is_ok());
    assert!(PeerContract::parse(br#"{"schema":1,"budget":1}"#).is_ok());
    assert!(PeerContract::parse(br#"{"schema":1,"budget":50000000}"#).is_ok());
}

#[test]
fn rejects_unbounded_or_retargeted_observation() {
    for bytes in [
        br#"{"schema":1,"budget":0}"#.as_slice(),
        br#"{"schema":1,"budget":50000001}"#.as_slice(),
        br#"{"schema":2,"budget":20000000}"#.as_slice(),
        br#"{"schema":1,"budget":20000000,"core":0}"#.as_slice(),
        br#"{"schema":1,"budget":20000000,"address":73728}"#.as_slice(),
    ] {
        assert!(PeerContract::parse(bytes).is_err());
    }
}

#[test]
fn accepts_only_the_two_source_bound_control_policies() {
    for control in ["held-sentinel", "delayed-release"] {
        let contract = format!(r#"{{"schema":2,"control":"{control}","budget":10000000}}"#);
        assert!(PeerContract::parse(contract.as_bytes()).is_ok());
    }
}

#[test]
fn rejects_control_budget_schema_and_site_changes() {
    for bytes in [
        br#"{"schema":2,"control":"held-sentinel","budget":9999999}"#.as_slice(),
        br#"{"schema":2,"control":"delayed-release","budget":10000001}"#.as_slice(),
        br#"{"schema":2,"control":"natural","budget":10000000}"#.as_slice(),
        br#"{"schema":1,"control":"held-sentinel","budget":10000000}"#.as_slice(),
        br#"{"schema":3,"control":"delayed-release","budget":10000000}"#.as_slice(),
        br#"{"schema":2,"control":"held-sentinel","budget":10000000,"core":0}"#.as_slice(),
        br#"{"schema":2,"control":"held-sentinel","budget":10000000,"waits":4}"#.as_slice(),
        br#"{"schema":2,"control":"delayed-release","budget":10000000,"gate":8180}"#.as_slice(),
        br#"{"schema":2,"control":"delayed-release","budget":10000000,"budget":10000000}"#.as_slice(),
        br#"{"schema":2,"control":"held-sentinel","control":"delayed-release","budget":10000000}"#.as_slice(),
        br#"{"schema":2,"control":"held-sentinel","budget":10000000.0}"#.as_slice(),
    ] {
        assert!(PeerContract::parse(bytes).is_err());
    }
}

fn owned(forbid_fetch: &str) -> String {
    format!(
        r#"{{"schema":3,"budget":20000000,"scenario":{{"kind":"natural","endpoint":"handoff"}},"program":{{"kind":"owned",
        "record_sha256":"{}","image":[66560,67936],
        "sites":{{"entry":66560,"main":66784,"peer_load":67704,"peer_loaded":67816,"clear_store":67804,"target_store":67748,
        "argument_store":67764,"target_load":66580,"argument_load":66588,"handoff":66592}},
        "loaded_register":0,"poll_period":512,"forbid_fetch":{forbid_fetch},
        "forbid_read":[[72388,72396],[72404,72424],[73700,73708]]}}}}"#,
        "ab".repeat(32)
    )
}

const FORBID_FETCH: &str = "[[71680,71764],[72164,72380],[72896,73700],[76128,76148],[76476,76496],[78992,79220],[79980,80044]]";

#[test]
fn accepts_retained_and_owned_startup_scenarios() {
    for scenario in [r#"{"kind":"natural","endpoint":"loader-entry"}"#, r#"{"kind":"natural","endpoint":"handoff"}"#] {
        let contract = format!(r#"{{"schema":3,"budget":20000000,"scenario":{scenario},"program":{{"kind":"retained"}}}}"#);
        assert!(PeerContract::parse(contract.as_bytes()).is_ok());
    }
    for scenario in [r#"{"kind":"held-sentinel"}"#, r#"{"kind":"delayed-release","endpoint":"handoff"}"#] {
        let contract = format!(r#"{{"schema":3,"budget":10000000,"scenario":{scenario},"program":{{"kind":"retained"}}}}"#);
        assert!(PeerContract::parse(contract.as_bytes()).is_ok());
    }
    assert!(PeerContract::parse(owned(FORBID_FETCH).as_bytes()).is_ok());
}

#[test]
fn rejects_retargeted_or_reduced_startup_guards() {
    let shifted = FORBID_FETCH.replace("[71680,71764]", "[71684,71764]");
    let omitted = FORBID_FETCH.replace("[71680,71764],", "");
    for contract in [
        owned(&shifted),
        owned(&omitted),
        owned(FORBID_FETCH).replace("\"budget\":20000000", "\"budget\":10000000"),
        owned(FORBID_FETCH).replace("\"handoff\":66592", "\"handoff\":66560"),
        owned(FORBID_FETCH).replace("\"image\":[66560,67936]", "\"image\":[65536,67936]"),
        owned(FORBID_FETCH).replace("\"loaded_register\":0", "\"loaded_register\":13"),
        r#"{"schema":3,"budget":10000000,"scenario":{"kind":"held-sentinel","endpoint":"handoff"},"program":{"kind":"retained"}}"#.into(),
        r#"{"schema":3,"budget":20000000,"scenario":{"kind":"natural"},"program":{"kind":"retained"}}"#.into(),
        r#"{"schema":3,"budget":20000000,"scenario":{"kind":"natural","endpoint":"handoff"},"program":{"kind":"retained","image":[0,4]}}"#.into(),
    ] {
        assert!(PeerContract::parse(contract.as_bytes()).is_err(), "{contract}");
    }
}

fn loader_program() -> String {
    format!(r#""program":{{"kind":"owned","record_sha256":"{digest}","record_immutable_sha256":"{digest}","loader_sha256":"{digest}",
        "handoff":66592,"entry":66604,"load":66700,"post_load":66704,"loaded_register":0,"poll_period":256,
        "image":[66560,68096],"refusal":null,
        "forbid_fetch":[[852000,852168],[854444,854480],[854596,854628]],"forbid_read":[[852168,852220]]}}"#,
        digest = "ab".repeat(32))
}

fn vector_install() -> String {
    format!(r#""completion_zero":{{"site":24596,"instruction":3851063296,"literal":24624}},
        "kernel_vector_install":{{"entry":262176,"installed":262280,
        "bootstrap":[3852464152,3852464152,3852464152,3852464152,3852464152,3852464152,3852464152,3852464152,24576,76,100,80,88,96,192,144],
        "code":[{{"span":[262176,262400],"sha256":"{digest}"}},{{"span":[272312,272340],"sha256":"{digest}"}}],
        "steps":[{{"site":262204,"slot":36,"before":76,"value":262400}},
        {{"site":262216,"slot":40,"before":100,"value":262580}},
        {{"site":262228,"slot":44,"before":80,"value":262440}},
        {{"site":262240,"slot":48,"before":88,"value":262472}},
        {{"site":262252,"slot":52,"before":96,"value":262504}},
        {{"site":262264,"slot":56,"before":192,"value":262660}},
        {{"site":262276,"slot":60,"before":144,"value":262532}}]}}"#,
        digest = "ab".repeat(32))
}

fn composed(scenario: &str) -> String {
    format!(r#"{{"schema":5,"budget":50000000,"scenario":{{"kind":"{scenario}" }},{program},
        "startup":{{"kind":"composed","record1_sha256":"{digest}","image_sha256":"{digest}",
        "symbols_sha256":"{digest}","source_sha256":"{digest}","image":[24576,25088],"entry":24576,"handoff":24608,
        "handoff_target":262176,"handoff_argument":6934008,"secondary_entry":66560,"secondary_image_sha256":"{digest}",
        "secondary_symbols_sha256":"{digest}","secondary_source_sha256":"{digest}",{vectors}}}}}"#,
        program = loader_program(), digest = "ab".repeat(32), vectors = vector_install())
}

#[cfg(not(feature = "scoped-watch"))]
#[test]
fn default_sdk_refuses_composed_startup() {
    for scenario in ["natural", "held", "delayed", "non-one"] {
        assert!(PeerContract::parse(composed(scenario).as_bytes()).is_err());
    }
}

#[cfg(feature = "scoped-watch")]
#[test]
fn scoped_sdk_admits_composed_controls_and_preserves_standalone_schema_four() {
    for scenario in ["natural", "held", "delayed", "non-one"] {
        assert!(PeerContract::parse(composed(scenario).as_bytes()).is_ok());
        let standalone = format!(r#"{{"schema":4,"budget":50000000,"scenario":{{"kind":"{scenario}"}},{}}}"#, loader_program());
        assert!(PeerContract::parse(standalone.as_bytes()).is_ok());
    }
}

#[cfg(feature = "scoped-watch")]
#[test]
fn composed_startup_rejects_role_identity_extent_and_guard_changes() {
    let valid = composed("natural");
    for invalid in [
        valid.replace("\"schema\":5", "\"schema\":4"),
        valid.replace("\"kind\":\"owned\"", "\"kind\":\"retained\""),
        valid.replace("\"kind\":\"natural\"", "\"kind\":\"forced\""),
        valid.replace("\"budget\":50000000", "\"budget\":49999999"),
        valid.replace("\"kind\":\"composed\"", "\"kind\":\"retained\""),
        valid.replace("\"entry\":24576", "\"entry\":6144"),
        valid.replace("\"entry\":24576", "\"entry\":24580"),
        valid.replace("\"handoff\":24608", "\"handoff\":25088"),
        valid.replace("\"handoff\":24608", "\"handoff\":24576"),
        valid.replace("\"handoff_target\":262176", "\"handoff_target\":262177"),
        valid.replace("\"handoff_argument\":6934008", "\"handoff_argument\":0"),
        valid.replace("\"secondary_entry\":66560", "\"secondary_entry\":71680"),
        valid.replace("\"image\":[24576,25088]", "\"image\":[24572,25088]"),
        valid.replace("\"image\":[24576,25088]", "\"image\":[24576,65540]"),
        valid.replace("\"image\":[24576,25088]", "\"image\":[24576,25087]"),
        valid.replace("\"image\":[66560,68096]", "\"image\":[66556,68096]"),
        valid.replace("\"image\":[66560,68096]", "\"image\":[66560,71684]"),
        valid.replace("\"secondary_source_sha256\":\"abab", "\"secondary_source_sha256\":\"cdcd"),
        valid.replace("\"symbols_sha256\":\"abab", "\"symbols_sha256\":\"ABAB"),
        valid.replace("[852000,852168],", ""),
        valid.replace("[852168,852220]", "[852172,852220]"),
        valid.replace("\"loaded_register\":0", "\"loaded_register\":3"),
        valid.replace("\"secondary_entry\":66560", "\"secondary_entry\":66560,\"core\":1"),
    ] {
        assert!(PeerContract::parse(invalid.as_bytes()).is_err(), "{invalid}");
    }
    let standalone = format!(r#"{{"schema":5,"budget":50000000,"scenario":{{"kind":"natural"}},{}}}"#, loader_program());
    assert!(PeerContract::parse(standalone.as_bytes()).is_err());
}

#[cfg(feature = "scoped-watch")]
#[test]
fn composed_requires_complete_zero_writer_and_seven_slot_installation_facts() {
    let valid = composed("held");
    let zero = r#""completion_zero":{"site":24596,"instruction":3851063296,"literal":24624},"#;
    for invalid in [
        valid.replace(zero, ""),
        valid.replace(zero, &(zero.to_owned() + zero)),
        valid.replace("\"completion_zero\":{", "\"completion_zero\":null,\"obsolete_zero\":{"),
        valid.replace("\"site\":24596", "\"site\":24597"),
        valid.replace("\"site\":24596", "\"site\":25088"),
        valid.replace("\"instruction\":3851063296", "\"instruction\":0"),
        valid.replace("\"instruction\":3851063296", "\"instruction\":3855261696"),
        valid.replace("\"literal\":24624", "\"literal\":24625"),
        valid.replace("\"literal\":24624", "\"literal\":25088"),
        valid.replace("\"kernel_vector_install\":{", "\"obsolete_vector_install\":{"),
        valid.replace("\"bootstrap\":", "\"obsolete_bootstrap\":"),
        valid.replace("\"code\":", "\"obsolete_code\":"),
        valid.replace("\"steps\":", "\"obsolete_steps\":"),
        valid.replace("\"installed\":262280", "\"installed\":262284"),
        valid.replace("\"entry\":262176", "\"entry\":262180"),
        valid.replace("[262176,262400]", "[262180,262400]"),
        valid.replace("[272312,272340]", "[272312,272336]"),
        valid.replace("\"sha256\":\"abab", "\"sha256\":\"ABAB"),
        valid.replace("\"sha256\":\"abab", "\"sha256\":\"zzzz"),
        valid.replace("24576,76,100,80", "24576,76,100"),
        valid.replace("\"site\":262204", "\"site\":262208"),
        valid.replace("\"slot\":36", "\"slot\":40"),
        valid.replace("\"before\":76", "\"before\":77"),
        valid.replace("\"value\":262400", "\"value\":262404"),
        valid.replace(r#"{"site":262204,"slot":36,"before":76,"value":262400},"#, ""),
        valid.replace(r#"{"site":262204,"slot":36,"before":76,"value":262400}"#, r#"{"site":262216,"slot":40,"before":100,"value":262580}"#),
    ] {
        assert!(PeerContract::parse(invalid.as_bytes()).is_err(), "{invalid}");
    }
}

#[cfg(feature = "scoped-watch")]
#[test]
fn standalone_schema_four_does_not_admit_composed_protocol_fields() {
    let valid = format!(r#"{{"schema":4,"budget":50000000,"scenario":{{"kind":"held"}},{}}}"#, loader_program());
    assert!(PeerContract::parse(valid.as_bytes()).is_ok());
    let invalid = valid.strip_suffix('}').unwrap().to_owned() + "," + &vector_install() + "}";
    assert!(PeerContract::parse(invalid.as_bytes()).is_err());
}

fn retained_loader() -> String {
    r#"{"schema":4,"budget":50000000,"scenario":{"kind":"natural"},"program":{"kind":"retained",
        "record_sha256":"2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4",
        "record_immutable_sha256":"0c10e1264bccdd36fe167cd4c22a3df48134945a710f2554817f9cafd76b83d0",
        "loader_sha256":"8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec",
        "handoff":71760,"entry":852000,"load":854596,"post_load":854600,
        "loaded_register":3,"poll_period":256,"image":null,"refusal":null,"forbid_fetch":[],"forbid_read":[]}}"#.into()
}

#[cfg(feature = "scoped-watch")]
#[test]
fn retained_loader_admission_binds_both_full_and_immutable_record_identity() {
    let valid = retained_loader();
    assert!(PeerContract::parse(valid.as_bytes()).is_ok());
    for invalid in [
        valid.replace("\"record_sha256\":\"2f1b", "\"record_sha256\":\"3f1b"),
        valid.replace("\"record_immutable_sha256\":\"0c10", "\"record_immutable_sha256\":\"1c10"),
        valid.replace("\"record_immutable_sha256\":\"0c10", "\"record_immutable_sha256\":\"0C10"),
    ] {
        assert!(PeerContract::parse(invalid.as_bytes()).is_err());
    }
}

#[cfg(feature = "scoped-watch")]
#[test]
fn schema_four_and_five_require_explicit_well_formed_immutable_identity() {
    let digest = "ab".repeat(32);
    let field = format!(r#""record_immutable_sha256":"{digest}","#);
    let standalone = format!(r#"{{"schema":4,"budget":50000000,"scenario":{{"kind":"natural"}},{}}}"#, loader_program());
    for valid in [standalone, composed("natural")] {
        for invalid in [
            valid.replace(field.as_str(), ""),
            valid.replace(field.as_str(), r#""record_immutable_sha256":null,"#),
            valid.replace(field.as_str(), r#""record_immutable_sha256":"ab","#),
            valid.replace(field.as_str(), &format!(r#""record_immutable_sha256":"{}","#, "AB".repeat(32))),
            valid.replace(field.as_str(), &(field.clone() + &field)),
            valid.replace("\"image\":[66560,68096]", "\"image\":[66564,68096]"),
            valid.replace("\"handoff\":66592", "\"handoff\":66560"),
        ] {
            assert!(PeerContract::parse(invalid.as_bytes()).is_err(), "{invalid}");
        }
    }
}

fn retained_initializer() -> String {
    format!(r#"{{"schema":6,"prefix":{},"initializer":{{"role":"retained","entry":{},"rust":[{},{}],
        "scratch_bytes":12,"image_sha256":"60aeb6c0c68717940fef3771f3387540a2c87276d7c947a9f3d276dd51fe850d",
        "symbols_sha256":null,"source_sha256":"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"}}}}"#,
        retained_loader(), 0xd0430, 0xd0430, 0xd09ac)
}

#[cfg(feature = "scoped-watch")]
fn owned_initializer() -> String {
    let prefix = format!(r#"{{"schema":4,"budget":50000000,"scenario":{{"kind":"natural"}},{}}}"#, loader_program());
    format!(r#"{{"schema":6,"prefix":{prefix},"initializer":{{"role":"owned","entry":{},"rust":[{},{}],
        "scratch_bytes":8,"image_sha256":"{digest}","symbols_sha256":"{digest}",
        "source_sha256":"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490"}}}}"#,
        0x10800, 0x10800, 0x10840, digest = "ab".repeat(32))
}

fn consumer_code() -> String {
    let regions: &[(u32, &[u32])] = &[
        (0x69cdf8, &[
            0xe92d4830,0xe28db00c,0xe24dd020,0xe3500000,0x1a00000d,0xebfee8dc,0xe3500001,0x1a000006,
            0xe3022e00,0xe3402035,0xe30e0c00,0xe3400089,0xe3a01000,0xe1a02122,0xeb0214da,0xeb0222f2,
            0xe3500000,0x0a00004c,0xeafffffe,0xe3500001,0x1a000010,0xe3a03068,0xe50b3028,0xe3013960,
            0xe3403078,0xe50b3024,0xe3a03001,0xe50b3020,0xe3a03b01,0xe50b301c,0xe30d3160,0xe3403069,
            0xe50b3018,0xe3a04000,0xe50b4014,0xe24b0028,0xeb0223d2,0xea000039,0xe3500002,0x030c4c6c,
            0x03404069,0x0a000035,0xe3500003,0x1a000010,0xe3a03066,0xe50b3028,0xe3013968,0xe3403078,
            0xe50b3024,0xe3a0300f,0xe50b3020,0xe3a03b01,0xe50b301c,0xe30c3bf0,0xe3403069,0xe50b3018,
            0xe3a04000,0xe50b4014,0xe24b0028,0xeb0223bb,0xea000022,0xe3500004,0x13a04000,0x1a00001f,
            0xe3a03067,0xe50b3028,0xe3013970,0xe3403078,0xe50b3024,0xe3a0300f,0xe50b3020,0xe3a05b01,
            0xe50b501c,0xe30c3bf0,0xe3403069,0xe50b3018,0xe3a04000,0xe50b4014,0xe24b0028,0xeb0223a7]),
        (0x725dd8, &[0xe92d48f0,0xe28db014,0xe24dd020,0xe1a04000,0xe30a3d9c,0xe340307c,
            0xe1d020b0,0xe2422001,0xe7936102,0xebfff5f5,0xe1a07000]),
        (0x7235d8, &[0xe92d4df0,0xe28db01c,0xe30b5d20,0xe3405096,0xe5952000,0xe3013618,
            0xe7923003,0xe12fff33,0xe1a04000,0xe7953100,0xe5933164,0xe12fff33,0xe1a0a000]),
        (0xd1140, &[0xe52db004,0xe28db000,0xe10f0000,0xe3801080,0xe121f001,0xe2000080,
            0xe28bd000,0xe8bd0800,0xe12fff1e]),
    ];
    format!("[{}]", regions.iter().map(|(start, words)|
        format!(r#"{{"start":{start},"words":{}}}"#, serde_json::to_string(words).unwrap())).collect::<Vec<_>>().join(","))
}

fn consumer_contract(initializer: &str) -> String {
    format!(r#"{{"schema":7,"initializer":{initializer},"consumer":{{"kind":"interrupt-mask",
        "source_sha256":"43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490",
        "loader_b_sha256":"8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec",
        "image4_sha256":"f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16","code":{}}}}}"#,
        consumer_code())
}

#[cfg(not(feature = "scoped-watch"))]
#[test]
fn ordinary_sdk_rejects_initializer_and_consumer_scopes() {
    let initializer = retained_initializer();
    assert!(PeerContract::parse(initializer.as_bytes()).is_err());
    assert!(PeerContract::parse(consumer_contract(&initializer).as_bytes()).is_err());
}

#[cfg(feature = "scoped-watch")]
#[test]
fn consumer_scope_preserves_both_initializer_roles_and_schema_six() {
    for initializer in [retained_initializer(), owned_initializer()] {
        assert!(PeerContract::parse(initializer.as_bytes()).is_ok());
        assert!(PeerContract::parse(consumer_contract(&initializer).as_bytes()).is_ok());
    }
}

#[cfg(feature = "scoped-watch")]
#[test]
fn consumer_scope_rejects_retargeted_source_code_and_nested_contracts() {
    let valid = consumer_contract(&retained_initializer());
    for invalid in [
        valid.replacen(r#""schema":7"#, r#""schema":6"#, 1),
        valid.replace(r#""schema":6"#, r#""schema":7"#),
        valid.replace(r#""schema":4"#, r#""schema":5"#),
        valid.replace(r#""kind":"interrupt-mask""#, r#""kind":"retained""#),
        valid.replace(r#""kind":"interrupt-mask""#, r#""kind":"irq-enable""#),
        valid.replace(r#""kind":"interrupt-mask""#, r#""kind":"interrupt-mask","entry":856384"#),
        valid.replace(r#""source_sha256":"43d6"#, r#""source_sha256":"53d6"#),
        valid.replace(r#""loader_b_sha256":"8fb0"#, r#""loader_b_sha256":"9fb0"#),
        valid.replace(r#""image4_sha256":"f2e2"#, r#""image4_sha256":"f2e3"#),
        valid.replace(&format!(r#""start":{}"#, 0xd1140), &format!(r#""start":{}"#, 0xd1144)),
        valid.replace(&0xe121f001_u32.to_string(), &0xe1a00000_u32.to_string()),
        valid.replace(&0xe12fff1e_u32.to_string(), "0"),
        valid.replace(r#""scratch_bytes":12"#, r#""scratch_bytes":8"#),
        valid.replace(&consumer_code(), "[]"),
        valid.replace(r#""kind":"natural""#, r#""kind":"held""#),
        valid.replace(r#""budget":50000000"#, r#""budget":49999999"#),
        valid.replace(r#""initializer":{"schema":6"#, r#""initializer":{"schema":6,"consumer":null"#),
        valid.replacen(r#""schema":7"#, r#""schema":7,"budget":50000000"#, 1),
        valid.replacen(r#""schema":7"#, r#""schema":7,"schema":7"#, 1),
        valid.replace(r#""words":["#, r#""extra":0,"words":["#),
    ] {
        assert!(PeerContract::parse(invalid.as_bytes()).is_err(), "{invalid}");
    }
    let without_initializer = format!(r#"{{"schema":7,"initializer":{},"consumer":{{"kind":"interrupt-mask"}}}}"#, retained_loader());
    assert!(PeerContract::parse(without_initializer.as_bytes()).is_err());
}
