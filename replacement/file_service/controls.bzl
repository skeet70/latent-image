"""Deliberately wrong owned file-service variants; each must fail a case after reaching the case exit."""

FILE_SERVICE_CONTROLS = [
    "sequence_wrap",
    "skip_allocated",
    "no_free_slot_advance",
    "release_after_failed_wait",
    "translate_unknown_ok",
    "readiness_absent_ok",
    "result_release_keeps_owner",
    "open_failure_keeps_tag",
    "defer_no_count",
    "intercept_keeps_callback",
    "media_task_no_busy",
    "media_task_worker_row",
    "close_keeps_tag",
    "open_failure_keeps_pair",
    "write_index_unstored",
    "flagged_close_unmarked",
    # search
    "search_output_uncleared",
    "search_keeps_rejected",
    "entry_size_unrounded",
    # volume
    "volume_attach_kind_unset",
    "volume_detach_callback_unrestored",
    "volume_format_slots_kept",
    "volume_reformat_state_unset",
    # counts
    "counts_extension_unchecked",
    "counts_flags_short",
    "position_without_directories",
    # selection
    "selection_odd_uncounted",
    "selection_tag_untruncated",
    "selection_selected_total_skipped",
    # catalog
    "catalog_group_mask_trimmed",
    "catalog_rebuild_selection_kept",
    "catalog_find_mark_kept",
    "catalog_complete_returns_status",
    "catalog_resolve_totals_inclusive",
    # paths
    "paths_cancel_restores_callback",
    "paths_request_callback_unrestored",
    "paths_directory_delete_returns_zero",
    "paths_filename_returns_status",
    "paths_remove_restores_callback",
    "paths_attribute_failure_unreloaded",
    # media_io
    "capacity_ignores_units",
    "pattern_single_repeat",
    "query_raw_status",
    "pages_range_inclusive",
    "pages_keep_state",
    "read_redirect_keeps_position",
]

def file_service_suffixes():
    return [""] + ["-control-" + control.replace("_", "-") for control in FILE_SERVICE_CONTROLS]
