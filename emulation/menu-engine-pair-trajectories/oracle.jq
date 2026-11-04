# A RAM-push trajectory for one --menu-observe oracle path (emulation/menu-oracle-NAME.json): the
# package prefix to the live view, the config's path walked over crawl-5, then its key; run as
#   jq -n -L emulation/menu-engine-pair-trajectories --arg role stock|owned --arg name NAME \
#     --slurpfile config emulation/menu-oracle-NAME.json \
#     --slurpfile graph private-data/scratch/ControllerStrategy/crawl-5/crawl-graph.json -f oracle.jq
include "engine";
$graph[0].graph as $g
| $config[0] as $c
| ($g.edges | map(select(.to != null) | {key: "\(.from):\(.key)", value: .to}) | from_entries) as $edge
| ($g.nodes | map({key: (.id | tostring), value: .}) | from_entries) as $node
| def menu($id): $node[$id | tostring].menu | [.menu_id, .setting_id, .cursor];
  def walk($from; $keys): reduce $keys[] as $k ({at: $from, steps: []};
      ($edge["\(.at):\($k)"]) as $to
      | if $to == null then error("crawl-5 has no \($k) edge from node \(.at)") else {at: $to, steps: (.steps + [[$k] + menu($to)])} end);
  # The installed package appends rows 33 and 34 to the shooting root (menu/recipe_bank_ui.h).
  def package_step: if . == ["up", 1, 5, 32] then ["up", 1, 5, 34] else . end;
  def reads: [["dispatcher-first-word", "00bfb028", 4], ["resident", "runtime:rb_resident", 1024], ["live-settings", "0096cc64", 450]] + engine_reads;
  def keep_resident: [{"read": "resident", "phase": "package-menu-prefix", "source_read": "resident"}];
  def pulse: {"hold_instructions": 8000000, "release_instructions": 8000000};
  def segment($phase; $capture; $steps): {"phase": $phase, "capture": $capture, "key_pulse": pulse, "steps": $steps, "reads": reads, "read_preservations": keep_resident};
  walk(0; $c.path + [$c.key]) as $walked
| ($walked.steps | map(package_step)) as $steps
| {
    "schema": 1,
    "entry": "Owned menu engine guest pair, \($role) role, for the menu-oracle-\($name) path: the package prefix uses MENU to reach the live view (67,56,0), crawl-5 root 0, then the --menu-observe path and key from emulation/menu-oracle-\($name).json with crawl-5's landings. Each segment reads the engine block, the last-move pair and the snapshot areas; the stage entry, kind switch, seek and event post are traced.",
    "source_bindings": [
      {"source": "emulation/menu-oracle-\($name).json", "expectation": "The path and key are the oracle case's."},
      {"source": "private-data/scratch/ControllerStrategy/crawl-5/crawl-graph.json (E-MODAL-MENU-OKPATHS-150)", "expectation": "Each step's menu tuple is the landing of the crawl-5 edge for that key from the previous step's screen."},
      {"source": "menu/recipe_bank_ui.h", "expectation": "With the package installed UP from (1,5,2) lands on row 34, not crawl-5's row 32."}
    ],
    "trace_sites": engine_sites($role),
    "before_unload": (
      [ {"phase": "package-menu-prefix", "capture": "oracle-live-view", "steps": [["menu", 67, 56, 0]], "reads": reads} ]
      + (if ($c.path | length) > 0 then [segment("oracle-path"; "oracle-path"; $steps[0:-1])] else [] end)
      + [segment("oracle-key"; "oracle-key"; $steps[-1:])]),
    "after_unload": []
  }
