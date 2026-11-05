# A RAM-push trajectory for one --menu-observe oracle path (emulation/menu-oracle-NAME.json): the
# package prefix to the live view, the config's path walked over crawl-5, then its key; run as
#   jq -n -L emulation/menu-engine-pair-trajectories --arg role stock|owned --arg name NAME \
#     --slurpfile config emulation/menu-oracle-NAME.json \
#     --slurpfile graph private-data/scratch/ControllerStrategy/crawl-5/crawl-graph.json \
#     [--argjson landing '[MENU, SETTING, CURSOR]'] [--argjson sample '[[KEY, MENU, SETTING, CURSOR], ...]'] -f oracle.jq
# `landing` is the key's screen from the oracle receipt's key reply, used where crawl-5's landing is
# not the one the receipt shows. `sample` is a segment before the path that opens and closes the
# shooting menu, for a path that dispatches no owned modal75 action, which the scoped lifetime guard
# needs for its task identity. Every trajectory ends with BACK to the live-view root, which the
# bridge's unload rendezvous requires.
include "engine";
$graph[0].graph as $g
| $config[0] as $c
| ($g.edges | map(select(.to != null) | {key: "\(.from):\(.key)", value: .to}) | from_entries) as $edge
| ($g.nodes | map({key: (.id | tostring), value: .}) | from_entries) as $node
| def menu($id): $node[$id | tostring].menu | [.menu_id, .setting_id, .cursor];
  def walk($from; $keys): reduce $keys[] as $k ({at: $from, steps: []};
      ($edge["\(.at):\($k)"]) as $to
      | if $to == null then error("crawl-5 has no \($k) edge from node \(.at)") else {at: $to, steps: (.steps + [[$k] + menu($to)])} end);
  # The installed package appends rows 33 and 34 to the shooting root (menu/recipe_bank_ui.h), so UP
  # from row 2 lands on row 34; a path that goes on from crawl-5's row 32 takes two more UPs to reach it.
  def package_route: . as $s | [range(0; $s | length) as $i | $s[$i]
      | if . != ["up", 1, 5, 32] then [.]
        elif $i == ($s | length) - 1 then [["up", 1, 5, 34]]
        else [["up", 1, 5, 34], ["up", 1, 5, 33], ["up", 1, 5, 32]] end] | add;
  def reads: [["dispatcher-first-word", "00bfb028", 4], ["resident", "runtime:rb_resident", 1024], ["live-settings", "0096cc64", 450]] + engine_reads;
  # The live-view root: controller 74 (the u16 at byte 148 of crawl-5's states) in state 4.
  def live($id): $node[$id | tostring].states[296:300] == "0400";
  # BACK from $from until the live-view root, landings from crawl-5. A BACK into the live view keeps
  # the menu reading of the screen it closed (the close and close-property receipts' key replies).
  def close_menu($from; $tuple): reduce range(0; 10) as $_ ({at: $from, tuple: $tuple, steps: []};
      if live(.at) then . else
        ($edge["\(.at):back"]) as $to
        | if $to == null then error("crawl-5 has no back edge from node \(.at)") else
            (if live($to) then .tuple else menu($to) end) as $t
            | {at: $to, tuple: $t, steps: (.steps + [["back"] + $t])} end end)
    | if live(.at) then .steps else error("ten BACKs from node \($from) do not reach the live view") end;
  def keep_resident: [{"read": "resident", "phase": "package-menu-prefix", "source_read": "resident"}];
  def segment($phase; $capture; $steps): {"phase": $phase, "capture": $capture, "steps": $steps, "reads": reads, "read_preservations": keep_resident};
  walk(0; $c.path) as $walked
| ($edge["\($walked.at):\($c.key)"]) as $keyed
| (if $ARGS.named.landing != null then [[$c.key] + $ARGS.named.landing]
   elif $keyed != null then [[$c.key] + menu($keyed)]
   else error("crawl-5 has no \($c.key) edge from node \($walked.at) and no landing was given") end) as $last
| ($walked.steps + $last | package_route) as $steps
| (if $keyed != null then $keyed
   elif $last[0][1:] == menu($walked.at) then $walked.at
   else error("the key's landing \($last[0][1:]) is neither a crawl-5 node nor the key's from-screen") end) as $landed
| close_menu($landed; $steps[-1][1:]) as $closing
| {
    "schema": 1,
    "entry": "Owned menu engine guest pair, \($role) role, for the menu-oracle-\($name) path: the package prefix starts at the live view (67,56,0), crawl-5 root 0, where the FujiMod rendezvous leaves it, then the --menu-observe path and key from emulation/menu-oracle-\($name).json with crawl-5's landings, then BACK to the live-view root. Each segment reads the engine block, the last-move pair and the snapshot areas; the stage entry, kind switch, seek and event post are traced.",
    "source_bindings": [
      {"source": "emulation/menu-oracle-\($name).json", "expectation": "The path and key are the oracle case's."},
      {"source": "private-data/scratch/ControllerStrategy/crawl-5/crawl-graph.json (E-MODAL-MENU-OKPATHS-150)", "expectation": "Each step's menu tuple is the landing of the crawl-5 edge for that key from the previous step's screen."},
      {"source": "menu/recipe_bank_ui.h", "expectation": "With the package installed UP from (1,5,2) lands on row 34, not crawl-5's row 32, and two more UPs reach row 32."}
    ] + (if $ARGS.named.landing != null then [{"source": "the menu-oracle-\($name) receipt's key reply", "expectation": "The key lands on \($ARGS.named.landing)."}] else [] end)
      + (if ($closing | length) > 0 then [{"source": "crawl-5's back edges and the menu-oracle-close and menu-oracle-close-property receipts' key replies", "expectation": "BACK returns to the live-view root (controller 74 in state 4) before the unload; a BACK into the live view keeps the menu reading of the screen it closes."}] else [] end)
      + (if $ARGS.named.sample != null then [{"source": "emulation/menu-oracle-open.json and emulation/menu-oracle-close.json receipts", "expectation": "OK from the live view opens the shooting menu on \($ARGS.named.sample[0][1:]) and BACK closes it on \($ARGS.named.sample[1][1:])."}] else [] end),
    "trace_sites": engine_sites($role),
    "before_unload": (
      [ {"phase": "package-menu-prefix", "capture": "oracle-live-view", "steps": [], "reads": reads} ]
      + (if $ARGS.named.sample != null then [segment("modal-sample"; "modal-sample"; $ARGS.named.sample)] else [] end)
      + (if ($c.path | length) > 0 then [segment("oracle-path"; "oracle-path"; $steps[0:-1])] else [] end)
      + [segment("oracle-key"; "oracle-key"; $steps[-1:])]
      + (if ($closing | length) > 0 then [segment("close-menu"; "close-menu"; $closing)] else [] end)),
    "after_unload": []
  }
