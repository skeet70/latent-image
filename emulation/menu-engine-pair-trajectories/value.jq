# Value-package pair inputs over the controlled-RTC trajectory inventory; run as
#   jq --arg role stock|owned [--arg route close-pop|change-stall|change-complete --arg receipt_source SOURCE]
#      -f value.jq TRAJECTORY
# Every route declares stock's IMAGE SIZE arm and stock's menu notification 0x00d5b350 and, in the
# owned role, the seam that replaces the arm. With --arg route, applied to picker-260.ROLE.json, it
# renders that IMAGE SIZE route on that picker's segment template: close-pop closes the unchanged
# picker; change-stall and change-complete change the selection and observe the 0x4010 submission
# and its completion (reports/owned-menu-engine-design.md#image-size-changed-selection).
def value_sites($role): [["00d51b58", "stock-value-size-arm"], ["00d5b350", "value-notify", {"lr": true}]]
  + (if $role == "owned" then [["runtime:rb_menu_value_size_apply", "owned-value-apply"]]
     elif $role == "stock" then [] else error("role must be stock or owned") end);
def close_pop:
  .before_unload as $segments
  | ($segments | map(.phase) | index("picker-route")) as $route
  | $segments[$route] as $template
  | if [$template.steps[] | .[0:4]] != [["ok",1,5,2],["down",1,5,3],["down",1,5,4],["down",1,5,5]]
      then error("close-pop renders on the picker-260 trajectory") else . end
  | def segment($name; $steps): $template | .phase = $name | .capture = $name | .steps = $steps;
  .entry = "IMAGE SIZE closed from inside its picker, then a restore elsewhere. OK opens the IMAGE SIZE picker (1,246,0), which enters stock's IMAGE SIZE arm 0x00d51b58 once; a half-press closes the menu from inside it (stage Close (2,1,0x1d), notified to 0x00d5b350 with class 0xf6). OK reopens SHOOTING MENU at row 3, two DOWNs reach row 5, OK opens picker 260, DOWN moves it to row 1 and BACK leaves it with Pop (0xf,1,0x1a), whose notification runs stock's restore. Reopening picker 260 lands on row 0, the restored value. No owned state may survive the IMAGE SIZE picker the half-press closed."
  | .source_bindings += [
      {"source": ($ARGS.named.receipt_source // error("close-pop needs --arg receipt_source")), "expectation": "On plain stock with the qualifier's run document every landing of this route matches, and the half-press inside the IMAGE SIZE picker posts stage (2,1,0x1d) and notifies (2,1,0x1d,0xf6)."},
      {"source": "replacement/menu_engine/adapter/mod.rs", "expectation": "Close and Pop reach stock's 0x00d5b350 notification in the owned role."}
    ]
  | .before_unload = $segments[0:$route] + [
      ($template | .steps = [["ok",1,5,2],["down",1,5,3]]),
      segment("value-open"; [["ok",1,246,0]]),
      segment("value-close"; [["half-press",1,246,0]]),
      segment("value-reopen"; [["ok",1,5,3]]),
      segment("other-route"; [["down",1,5,4],["down",1,5,5]]),
      segment("other-open"; [["ok",1,260,0]]),
      segment("other-move"; [["down",1,260,1]]),
      segment("other-cancel"; [["back",1,5,5]]),
      segment("other-reopen"; [["ok",1,260,0]]),
      segment("other-leave"; [["back",1,5,5]]),
      ($segments[] | select(.phase == "close-menu"))
    ];
def change_sites: [
  ["0103a254", "pipeline-4010-submit", {"lr": true}],
  ["01029188", "pipeline-4010-receive", {"reads": [["016fea28", 48], ["016fe9f8", 4], ["017030d0", 28], ["016fec4c", 4]]}],
  ["01029430", "pipeline-4010-complete", {"reads": [["017030d0", 4], ["016fe9f8", 4]]}],
  ["00f3a834", "pipeline-4010-callback", {"lr": true}],
  ["00defa30", "record-13a5-send", {"reads": [{"sp": 0, "bytes": 4}]}],
  ["00d3680c", "menu-token-release", {"reads": [["0094f890", 8]]}]];
def change_reads: [
  ["pipeline-pending", "017030d0", 28], ["pipeline-callback", "016fe9f8", 4], ["pipeline-state", "016fec4c", 4],
  ["pipeline-completion-mask", "01698d48", 8], ["menu-token-bitmaps", "0094f888", 48]];
def change($kind):
  .before_unload as $segments
  | ($segments | map(.phase) | index("picker-route")) as $route
  | $segments[$route] as $template
  | if [$template.steps[] | .[0:4]] != [["ok",1,5,2],["down",1,5,3],["down",1,5,4],["down",1,5,5]]
      then error("change routes render on the picker-260 trajectory") else . end
  | def segment($name; $steps): $template | .phase = $name | .capture = $name | .steps = $steps
      | .reads += change_reads;
    def observed($name; $steps): segment($name; $steps) | .observe = true;
    def idle: {"idle_instructions": 20000000};
    def prompt: {"settle_instructions": 1};
  .trace_sites += change_sites
  | if $kind == "stall" then
      .entry = "IMAGE SIZE changed under the qualifier's run document. OK opens the IMAGE SIZE picker (1,246,0) and DOWN moves it to row 1, which submits pipeline command 0x4010 with a completion callback. The pipeline never reserves both result groups, so the submission stays pending; the half-press, OK, a second DOWN and BACK are observed, not asserted, and the route ends with the menu open, so unload is refused in both roles."
      | .source_bindings += [
          {"source": "private-data/scratch/MenuValueFix/runs/20261113T184735Z-diag-change-close-half", "expectation": "Plain stock refuses the half-press after DOWN and keeps controller 74 open."},
          {"source": "reports/full-system-emulation.md#1110-image-size-pipeline-event-boundary", "expectation": "0x01029188 holds the callback pending until 0x01029430 invokes it with zero."}]
      | .before_unload = $segments[0:$route] + [
          ($template | .steps = [["ok",1,5,2],["down",1,5,3]] | .reads += change_reads),
          segment("value-open"; [["ok",1,246,0]]),
          observed("value-change"; [["down",1,246,1]]),
          observed("value-close"; [["half-press",1,246,1,idle]]),
          observed("value-confirm"; [["ok",1,246,1,idle]]),
          observed("value-change-again"; [["down",1,246,2,idle]]),
          observed("value-back"; [["back",1,5,3,idle]])]
      | .after_unload = []
    elif $kind == "complete" then
      .entry = "IMAGE SIZE changed under a run document whose pipeline schedule completes 0x4010. A completion arrives before a half-press Close, before an OK confirmation, and, with prompt settles, possibly after a half-press; a restore elsewhere follows. Landings after the first DOWN are observed, not asserted."
      | .source_bindings += [
          {"source": "private-data/scratch/AfKeyState/report-AfPoolPhase.md", "expectation": "Under the shifted schedules plain stock completes 0x4010, sends 0x13a5 and releases the token after DOWN in the IMAGE SIZE picker."}]
      | .before_unload = $segments[0:$route] + [
          ($template | .steps = [["ok",1,5,2],["down",1,5,3]] | .reads += change_reads),
          segment("value-open"; [["ok",1,246,0]]),
          observed("value-change"; [["down",1,246,1]]),
          observed("value-close"; [["half-press",1,246,1,idle]]),
          observed("value-reopen"; [["ok",1,5,3]]),
          observed("value-open-confirm"; [["ok",1,246,1]]),
          observed("value-change-confirm"; [["down",1,246,2]]),
          observed("value-confirm"; [["ok",1,5,3,idle]]),
          observed("value-open-race"; [["ok",1,246,2]]),
          observed("value-change-race"; [["down",1,246,3,prompt]]),
          observed("value-race-close"; [["half-press",1,246,3,prompt]]),
          observed("value-race-settle"; [["half-press",1,246,3,idle]]),
          observed("value-reopen-race"; [["ok",1,5,3]]),
          observed("other-route"; [["down",1,5,4],["down",1,5,5]]),
          observed("other-open"; [["ok",1,260,0]]),
          observed("other-move"; [["down",1,260,1]]),
          observed("other-cancel"; [["back",1,5,5]]),
          observed("other-reopen"; [["ok",1,260,0]]),
          observed("other-leave"; [["back",1,5,5]]),
          ($segments[] | select(.phase == "close-menu"))]
    else error("change kind must be stall or complete") end;
(if $ARGS.named.route == "close-pop" then close_pop
 elif $ARGS.named.route == "change-stall" then change("stall")
 elif $ARGS.named.route == "change-complete" then change("complete")
 elif $ARGS.named.route == null then .
 else error("route must be close-pop, change-stall, change-complete or absent") end)
| .trace_sites += value_sites($role)
