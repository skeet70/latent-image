use fujimod_xm1::checkpoint::BootIdentity;

#[test]
fn matching_boot_inputs_find_saved_checkpoint_and_changes_miss() {
    let root = std::env::temp_dir().join(format!("xm1-checkpoint-keys-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let identity = |run: &[u8], dat: &[u8], rom: &[u8], nand: &[u8], card: Option<&str>, procedure: &[u8]| {
        BootIdentity::new(run, dat, rom, nand, card, procedure)
    };
    let original = identity(b"run", b"dat", b"rom", b"nand", Some("card"), b"attach");
    let recorded = serde_json::to_vec(&original.document()).unwrap();
    assert_eq!(original.key().as_str(), fujimod_xm1::bridge::sha256(&recorded));
    let saved = root.join(original.key().as_str());
    std::fs::create_dir_all(&saved).unwrap();
    assert!(root.join(identity(b"run", b"dat", b"rom", b"nand", Some("card"), b"attach").key().as_str()).is_dir());
    for changed in [
        identity(b"changed", b"dat", b"rom", b"nand", Some("card"), b"attach"),
        identity(b"run", b"changed", b"rom", b"nand", Some("card"), b"attach"),
        identity(b"run", b"dat", b"changed", b"nand", Some("card"), b"attach"),
        identity(b"run", b"dat", b"rom", b"changed", Some("card"), b"attach"),
        identity(b"run", b"dat", b"rom", b"nand", Some("changed"), b"attach"),
        identity(b"run", b"dat", b"rom", b"nand", None, b"attach"),
        identity(b"run", b"dat", b"rom", b"nand", Some("card"), b"changed"),
    ] {
        assert!(!root.join(changed.key().as_str()).exists());
    }
    std::fs::remove_dir_all(root).unwrap();
}
