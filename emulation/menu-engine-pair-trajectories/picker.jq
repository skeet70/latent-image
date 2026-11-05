# A modal75 picker trajectory (emulation/modal75-picker-trajectories) with the engine pair's reads
# on every segment and its trace sites; run as
#   jq -L emulation/menu-engine-pair-trajectories --arg role stock|owned -f picker.jq picker-N.json
# The trajectory leaves the picker on the shooting root (1,5,r). A BACK there returns to the live-view
# root before the unload, which the bridge's unload rendezvous requires, and keeps the menu reading
# (the menu-oracle-close receipt's key reply). The rendezvous's UP hold opens the focus-area overlay
# (crawl-5's edge from live node 143 to node 147, controller 74 in state 0x11). After the unload the
# first OK leaves the overlay with the reading kept (crawl-5's edge 147 to 238, state 4) and the second
# reopens the shooting root on the row the reading names (crawl-5's edge from node 143, read as
# (1,5,15), to node 136 on (1,5,15)).
include "engine";
(.before_unload[-1].steps[-1][1:4]) as $root
| if $root[0:2] != [1, 5] then error("the picker trajectory does not end on the shooting root: \($root)") else . end
| .trace_sites += engine_sites($role)
| .before_unload += [.before_unload[-1] | .phase = "close-menu" | .capture = "close-menu" | .steps = [["back"] + $root]]
| .after_unload = [.after_unload[0] | del(.observe, .read_expectations) | .phase = "reopen-menu" | .capture = "reopen-menu" | .steps = [["ok"] + $root, ["ok"] + $root]] + .after_unload
| .source_bindings += [
    {"source": "private-data/scratch/ControllerStrategy/inc3/close receipt (emulation/menu-oracle-close.json)", "expectation": "BACK from the shooting root returns to the live view and keeps the menu reading \($root)."},
    {"source": "private-data/scratch/ControllerStrategy/crawl-5/crawl-graph.json (E-MODAL-MENU-OKPATHS-150)", "expectation": "UP at the live-view root opens the focus-area overlay (edge 143 to 147, controller 74 in state 0x11); the first OK leaves it for the live view with the menu reading kept (edge 147 to 238); the second OK reopens the shooting root on the row the menu reading names (edge 143 to 136)."}]
| .before_unload |= map(.reads += engine_reads)
| .after_unload |= map(.reads += engine_reads)
