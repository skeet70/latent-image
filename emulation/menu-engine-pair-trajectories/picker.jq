# A modal75 picker trajectory (emulation/modal75-picker-trajectories) with the engine pair's reads
# on every segment and its trace sites; run as
#   jq -L emulation/menu-engine-pair-trajectories --arg role stock|owned -f picker.jq picker-N.json
# The picker file already closes SHOOTING MENU before the unload and reopens it after.
include "engine";
.trace_sites += engine_sites($role)
| .before_unload |= map(.reads += engine_reads)
| .after_unload |= map(.reads += engine_reads)
| render_trajectory
