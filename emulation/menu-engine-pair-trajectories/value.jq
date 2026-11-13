# Value-package pair inputs over the controlled-RTC trajectory inventory; run as
#   jq --arg role stock|owned [--arg route close-pop] -f value.jq TRAJECTORY
# Every route declares stock's IMAGE SIZE arm and, in the owned role, the seam that replaces it.
# With --arg route close-pop, applied to picker-260.ROLE.json, it renders the IMAGE SIZE
# close-then-pop route on that picker's segment template.
def value_sites($role): [["00d51b58", "stock-value-size-arm"]]
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
      {"source": "private-data/scratch/MenuValueFix/runs/20261113T174655Z-diag-close-half/result", "expectation": "On plain stock with the qualifier's run document every landing of this route matches, and the half-press inside the IMAGE SIZE picker posts stage (2,1,0x1d) and notifies (2,1,0x1d,0xf6)."},
      {"source": "replacement/menu_engine/adapter/mod.rs", "expectation": "Close and Pop reach stock's 0x00d5b350 notification in the owned role."}
    ]
  | .trace_sites += [["00d5b350", "value-notify", {"lr": true}]]
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
(if $ARGS.named.route == "close-pop" then close_pop
 elif $ARGS.named.route == null then .
 else error("route must be close-pop or absent") end)
| .trace_sites += value_sites($role)
