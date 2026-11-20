"""The one fixed bank's linked variants.

Every variant links Root, the flag/mailbox family, the task60 loading owner, the file service and
the display scanout in one image; a non-empty suffix changes one unit. Fields: rustc cfgs of the
bank root, rlib overrides, linker entry, testonly.
"""

load("//replacement/file_service:controls.bzl", "FILE_SERVICE_CONTROLS")

_DISPATCH = "xm1_saclac_dispatch_entry"
_DRIVER = "xm1_saclac_driver_entry"

def _variant(cfgs = [], deps = {}, entry = _DISPATCH, testonly = False):
    return struct(cfgs = cfgs, deps = deps, entry = entry, testonly = testonly)

BANK_VARIANTS = dict(
    [
        ("", _variant()),
        ("-wrong-r3", _variant(cfgs = ["saclac_wrong_r3"])),
        ("-wrong-driver-store", _variant(cfgs = ["saclac_wrong_driver_store"], entry = _DRIVER)),
        # The defect is the cfg-compiled root rlib's `publish_history` index advance.
        ("-wrong-wake-publication", _variant(
            cfgs = ["saclac_wrong_wake_publication"],
            deps = {"xm1_saclac_root": "//replacement/saclac/root:root-wrong-wake-publication"},
            entry = _DRIVER,
        )),
        ("-wrong-driver-playback", _variant(cfgs = ["saclac_wrong_driver_playback"], entry = _DRIVER)),
        # The loading owner's retention control: a false quiescence drain and an explicit test timeout.
        ("-retention-control", _variant(
            deps = {"xm1_loading_policy": "//replacement/executable_loading:policy-retention-root"},
            testonly = True,
        )),
    ] + [
        ("-control-" + control.replace("_", "-"), _variant(
            deps = {"xm1_file_service": "//replacement/file_service:arm-control-" + control.replace("_", "-")},
        ))
        for control in FILE_SERVICE_CONTROLS
    ] + [
        ("-" + defect, _variant(deps = {"xm1_display_scanout": "//replacement/display_scanout:fixed-bank-" + defect}))
        for defect in ["register-order", "skipped-drain"]
    ],
)
