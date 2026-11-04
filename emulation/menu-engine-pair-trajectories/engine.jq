# The owned menu engine pair's reads and trace sites (reports/owned-menu-engine-design.md, Verification).
# Labels are those of replacement/modal_runtime/Replacement/ModalRuntime/MenuEnginePair.hs.
# The depth-1 and depth-2 snapshot areas (0x86 slots of 0x2c bytes) are each read in two halves of 0x43 slots.
def engine_reads: [
  ["menu-block", "009571d0", 1072],
  ["menu-last-move", "01467580", 8],
  ["snapshot-root", "00944414", 44],
  ["snapshot-depth1-0", "0093eb74", 2948],
  ["snapshot-depth1-1", "0093f6f8", 2948],
  ["snapshot-depth2-0", "009412d4", 2948],
  ["snapshot-depth2-1", "00941e58", 2948]
];
# The stage entry reads the block and the last-move pair at each event; the kind switch's and the
# seek's lr tell 0xbf95c8's call (and the owned fallback, which resumes there) from the adapter's
# service calls.
def engine_sites($role): [
  ["00d4dd2c", "menu-stage-entry", {"reads": [["009571d0", 1072], ["01467580", 8]]}],
  ["00bfa234", "menu-kind-switch", {"lr": true}],
  ["00bfad04", "menu-seek", {"lr": true}],
  ["0063270c", "menu-event-post"]
] + (if $role == "owned" then [["runtime:rb_menu_engine_guard_failure", "menu-engine-guard-failure"]]
     elif $role == "stock" then [] else error("role must be stock or owned") end);
