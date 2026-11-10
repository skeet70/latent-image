PROPERTY_STORAGE_CONTROLS = {
    "wrong-bank-stride": "storage_defect_wrong_bank_stride",
    "slot-projections-independent": "storage_defect_slot_projections_independent",
    "wrapped-selector-rejected": "storage_defect_wrapped_selector_rejected",
    "save-on-bank1": "storage_defect_save_on_bank1",
    "halfword-default-truncated": "storage_defect_halfword_default_truncated",
    "snapshot-entire-bank-per-set": "storage_defect_snapshot_entire_bank_per_set",
    "second-save-omitted": "property_defect_drop_second_save",
    "save-after-final-store": "storage_defect_save_after_final_store",
    "e9-record0-mirror-omitted": "storage_defect_e9_record0_mirror_omitted",
    "read-bank-before-refresh": "storage_defect_read_bank_before_refresh",
    "close-before-field-store": "storage_defect_close_before_field_store",
    "bracket-bank1": "storage_defect_bracket_bank1",
    "pair-close-after-both-writes": "storage_defect_pair_close_after_both_writes",
    "checksum-written-into-bank": "storage_defect_checksum_written_into_bank",
    "close-appends-area3": "storage_defect_close_appends_area3",
    "handover-reinitialize-all-banks": "storage_defect_handover_reinitialize_all_banks",
    "mirror-private-copy-diverges": "storage_defect_mirror_private_copy_diverges",
}

PROPERTY_STORAGE_VARIANTS = dict(PROPERTY_STORAGE_CONTROLS, **{"refusal": "storage_fixture_refuse_requests"})

BANK_STORAGE_CONTROLS = [
    "restore-without-body-write",
    "load-whole-bank-from-area3",
    "zero-loader-gaps",
    "tail-loaded-from-record",
    "initialize-only-active-bank",
    "skip-startup-resave",
    "body-write-on-unassigned",
    "body-not-refreshed",
    "application-appends-bank-as-area3",
    "commit-clears-previous",
]

CUSTOM_NATIVE_DEFECTS = ["accept-high-row-bits", "drop-recovery"]
CUSTOM_JOINED_DEFECTS = ["record-append-omitted", "record-crc-omitted"]
CUSTOM_ORACLE_DEFECTS = CUSTOM_NATIVE_DEFECTS + CUSTOM_JOINED_DEFECTS + ["unlock-before-copy"]
CUSTOM_STORAGE_VARIANTS = [struct(name = "", flag = None, record = ":record-store-guest-rlib")] + [
    struct(name = defect, flag = "custom_control_" + defect.replace("-", "_"), record = ":record-store-guest-rlib")
    for defect in CUSTOM_NATIVE_DEFECTS + CUSTOM_JOINED_DEFECTS
] + [
    struct(name = "bank-" + defect, flag = "storage_defect_" + defect.replace("-", "_"), record = ":record-store-guest-rlib")
    for defect in BANK_STORAGE_CONTROLS
] + [
    struct(name = "unlock-before-copy", flag = None, record = ":record-store-guest-rlib-unlock-before-copy"),
]
