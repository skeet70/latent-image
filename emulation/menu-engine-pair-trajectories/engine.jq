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
  ] + (if $name == "framed-pop-group41" then [
    ["011130e0","render-fill-entry"],
    ["01113514","render-pane-cell-box",{"sp":true,"reads":[{"sp":0,"bytes":12}]}],
    ["0111278c","render-pane-aligned"],
    ["011121d4","render-pane-label-grid"],
    ["0111236c","render-pane-grid"],
    ["01112d64","render-pane-bounds"]
  ] else [] end)
    + (if $role == "owned" then [
    ["runtime:rb_render_tree_entry","render-tree-adapter",{"sp":true}],
    ["runtime:rb_render_picker_entry","render-picker-adapter",{"sp":true}],
    ["runtime:rb_render_pane_entry","render-pane-adapter",{"sp":true}]
  ] else [] end) else [] end;

# Frame publication observation (FramePresentation.hs), enabled by `--argjson frame_objects [O(0,0), O(0,1)]`:
# the stage-0 buffer objects whose +0x1c words are the two display images. `frame_record` is the owned role's
# rb_render_frame_record address (render-frame-record.json of the installed package); that package's
# rb_render_frame_scanout_bridge registers owned frames' display completion in place of 0x6d80f0.
def frame_objects: $ARGS.named.frame_objects // null;
def frame_record: if $ARGS.named.role == "owned" then $ARGS.named.frame_record // null else null end;
def frame_enabled:
  if frame_objects == null then false
  elif frame_objects | type == "array" and length == 2 and all(.[]; type == "number" and . == floor and . > 0) then true
  else error("frame_objects must be the two stage-0 object addresses") end;
def frame_step_reads: [
  ["008e21f8", 1], ["008e2158", 8],
  [(frame_objects[0] + 28 | hex8), 4], [(frame_objects[1] + 28 | hex8), 4],
  ["0096bc10", 4]
] + (if frame_record != null then [[(frame_record | hex8), 120]] else [] end);
def frame_sites: if frame_enabled then [
  ["006d7920", "frame-step-entry", {"lr": true, "reads": frame_step_reads}],
  ["006d6f08", "frame-dispatch-return", {"reads": frame_step_reads}],
  ["006d7da4", "frame-wait-return"],
  ["006c9cf4", "frame-item-configuration", {"lr": true}],
  ["006d80f0", "frame-display-registration"],
  ["006d7f30", "frame-display-post"]
] + (if frame_record != null then [["runtime:rb_render_frame_scanout_bridge", "frame-owned-display-registration"]] else [] end) else [] end;

# Stage-0 submission observation (FrameSubmission.hs), enabled by `--argjson submission true`: the retained OpenVG
# and EGL entries VGLIB's render 0x6d724c calls, with the arrays their pointer arguments name, and the vector-engine
# job boundary: each 104-byte payload at graphics_send_resource_signal 0x892054 and its result at 0x8920ac, each
# 112-byte command task 72 dispatches at 0x8929f4, and the interrupt status 0x992934 when 0x892718 wakes.
def submission_enabled: $ARGS.named.submission == true;
def register_read($register; $bytes): {"register": $register, "offset": 0, "bytes": $bytes};
def submission_sites: if submission_enabled then [
  ["00858c04", "vg-set-integer"],
  ["00858b78", "vg-set-float"],
  ["00858c90", "vg-set-vector", {"reads": [register_read(2; 32)]}],
  ["00858d24", "vg-set-integers", {"reads": [register_read(2; 16)]}],
  ["0085a1b0", "vg-clear"],
  ["00858748", "vg-load-identity"],
  ["008587c4", "vg-load-matrix", {"reads": [register_read(0; 36)]}],
  ["008588cc", "vg-multiply-matrix", {"reads": [register_read(0; 36)]}],
  ["00858950", "vg-translate"],
  ["00858af4", "vg-rotate"],
  ["008589dc", "vg-scale"],
  ["0085afbc", "vg-set-paint"],
  ["0085ab88", "vg-draw-path"],
  ["0085b930", "vg-draw-image"],
  ["008586d8", "vg-finish"],
  ["00858e60", "vg-get-integer"],
  ["0088bf94", "egl-make-current", {"reads": [{"register": 13, "offset": 0, "bytes": 44, "indirect": true}]}],
  ["0088d620", "egl-copy-buffers"],
  ["00887280", "egl-get-error"],
  ["00892054", "vg-job-submit", {"reads": [register_read(0; 104)]}],
  ["008920ac", "vg-job-submit-return"],
  ["008929f4", "vg-job-dispatch", {"reads": [register_read(0; 112)]}],
  ["00892728", "vg-job-completion", {"reads": [["00992934", 4]]}]
] else [] end;
def frame_capture_reads: if frame_enabled then
  [["frame-display-record", "0095a69c", 20]]
  + (if frame_record != null then [["frame-record", (frame_record | hex8), 120]] else [] end)
  else [] end;
def render_reads: (if render_enabled then render_capture_reads else [] end) + frame_capture_reads;

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
     elif $role == "stock" then [] else error("role must be stock or owned") end) + render_sites($role; $name) + frame_sites + submission_sites;
def presented_handoff:
  if length == 0 then . else .[-1] |= (if length == 4 then . + [{completion:"presented-frame"}] else .[4].completion = "presented-frame" end) end;
def render_trajectory:
  if render_enabled then .before_unload = [
    {phase:"render-baseline", steps:[], reads:engine_reads}
  ] + .before_unload else . end
  | .before_unload |= map(if .phase == "close-menu" then .steps |= presented_handoff else . end);
