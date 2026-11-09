def render_enabled: $ARGS.named.render == true;
def hex8:
  . as $n | "0123456789abcdef" as $digits
  | [range(0;8) as $i | $digits[($n / pow(16;7-$i) | floor) % 16:][0:1]] | join("");
def render_banks:
  ($ARGS.named.render_banks // [range(0;19)])
  | if type != "array" or any(.[]; type != "number" or . != floor or . < 0 or . >= 19) or (unique | length) != length
    then error("render_banks must be distinct bank indices 0..18") else . end;
def render_stack_base: $ARGS.named.render_stack_base // 10800640;
def render_capture_reads: [
  ["render-osd-front", "008e21f8", 1], ["render-stage0-images", "008e2158", 8],
  ["render-task-node", "40060614", 4], ["render-task-record", "4005c3ac", 40],
  ["render-task-stack", (render_stack_base | hex8), 4096]
];
def render_entry_reads: [{"sp":0,"bytes":28}, ["009571d0",1072], ["015e64d8",24], ["015e46c0",4], ["015e748c",152], ["40054760",4], ["40060614",4], ["4005c3ac",40]];
def chunks($base; $bytes): [range(0;$bytes;4096) as $offset | [($base + $offset | hex8), ([4096, $bytes - $offset] | min)]];
def render_return_reads($offset):
  [{"sp":$offset,"bytes":68}, ["015e64d8",24], ["015e6680",64], ["015e748c",152]]
  + chunks(9801084;4800)
  + [render_banks[] | chunks(22967608 + . * 27136;27136)[]];
def render_pane_entry_reads: render_entry_reads | .[2] = ["015e64d8",40];
def render_pane_return_reads: render_return_reads(0) | .[0] = ["015e650c",68] | .[1] = ["015e64d8",40];
def render_sites($role; $name):
  if render_enabled then [
    ["01164e1c","render-tree-entry",{"lr":true,"sp":true,"reads":render_entry_reads}],
    ["01163f2c","render-picker-entry",{"lr":true,"sp":true,"reads":render_entry_reads}],
    ["011662a0","render-pane-entry",{"lr":true,"sp":true,"reads":render_pane_entry_reads}],
    ["01167a60","render-tree-return",{"sp":true,"reads":render_return_reads(196)}],
    ["01168ce4","render-picker-return",{"sp":true,"reads":render_return_reads(156)}],
    ["0116c588","render-picker-fn-return",{"sp":true,"reads":render_return_reads(76)}],
    ["01164e20","render-tree-stock"], ["01163f30","render-picker-stock"],
    ["011662a4","render-pane-stock"],
    ["01158184","render-pane-return",{"sp":true,"reads":render_pane_return_reads}],
    ["006d7d44","render-osd-increment"],
    ["006d76ec","render-osd-buffer",{"lr":true}]
  ] + (if $name == "framed-pop-group41" then [["011130e0","render-picker-fill"]] else [] end)
    + (if $role == "owned" then [
    ["runtime:rb_render_tree_entry","render-tree-adapter",{"sp":true}],
    ["runtime:rb_render_picker_entry","render-picker-adapter",{"sp":true}],
    ["runtime:rb_render_pane_entry","render-pane-adapter",{"sp":true}]
  ] else [] end) else [] end;
def render_reads: if render_enabled then render_capture_reads else [] end;

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
  ["snapshot-depth2-1", "00941e58", 2948],
  # Task 1's whole stack, for the checker's measured high-water mark; not compared between roles.
  ["task1-stack", "00a4c200", 3072]
] + render_reads;
# The stage entry reads the block and the last-move pair and keeps sp at each event; the kind
# switch's and the seek's lr tell 0xbf95c8's call (and the owned fallback, which resumes there)
# from the adapter's service calls.
def engine_sites($role; $name): [
  ["00d4dd2c", "menu-stage-entry", {"reads": [["009571d0", 1072], ["01467580", 8]], "sp": true}],
  ["00bfa234", "menu-kind-switch", {"lr": true}],
  ["00bfad04", "menu-seek", {"lr": true}],
  ["0063270c", "menu-event-post"]
] + (if $role == "owned" then [["runtime:rb_menu_engine_guard_failure", "menu-engine-guard-failure"]]
     elif $role == "stock" then [] else error("role must be stock or owned") end) + render_sites($role; $name);
def presented_handoff:
  if length == 0 then . else .[-1] |= (if length == 4 then . + [{completion:"presented-frame"}] else .[4].completion = "presented-frame" end) end;
def render_trajectory:
  if render_enabled then .before_unload = [
    {phase:"render-baseline", steps:[], reads:engine_reads}
  ] + .before_unload else . end
  | .before_unload |= map(if .phase == "close-menu" then .steps |= presented_handoff else . end);
