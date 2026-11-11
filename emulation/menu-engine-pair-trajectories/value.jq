include "engine";
def value_sites($role): [
  ["00d51b58", "stock-value-size-arm"],
  ["0061c93c", "value-property-read"],
  ["0061c91c", "value-property-write"],
  ["00d5b350", "value-notify", {"lr":true}]
] + (if $role == "owned" then [
  ["runtime:rb_menu_value_begin", "owned-value-begin"],
  ["runtime:rb_menu_value_confirm", "owned-value-confirm"],
  ["runtime:rb_menu_value_cancel", "owned-value-cancel"],
  ["runtime:rb_menu_value_size_apply", "owned-value-apply"],
  ["runtime:rb_menu_value_retained", "owned-value-handback"]
] else [] end);
.entry = "Installed IMAGE SIZE lifecycle: move and LEFT accept, reopen the accepted row, move and BACK cancel, reopen the accepted row, UP and OK accept, RIGHT stays open, BACK handback, unload and stock reopen."
| .trace_sites += engine_sites($role) + value_sites($role)
| .before_unload[2] as $picker
| .before_unload = .before_unload[0:2] + ([
    ["open", ["ok", 1, 246, 0]],
    ["move", ["down", 1, 246, 1]],
    ["accept-left", ["left", 1, 5, 3]],
    ["reopen-accepted", ["ok", 1, 246, 1]],
    ["move-before-cancel", ["down", 1, 246, 2]],
    ["cancel", ["back", 1, 5, 3]],
    ["reopen-cancelled", ["ok", 1, 246, 1]],
    ["move-up", ["up", 1, 246, 0]],
    ["accept-ok", ["ok", 1, 5, 3]],
    ["reopen-ok", ["ok", 1, 246, 0]],
    ["right-stays-open", ["right", 1, 246, 0]],
    ["handback", ["back", 1, 5, 3]]
  ] | map(. as [$name, $step] | $picker | .phase = ("value-" + $name) | .capture = .phase | .steps = [$step])) + [.before_unload[-1]]
| .before_unload |= map(.reads += engine_reads + [["value-size-dispatch", "00d50084", 4], ["value-working-cells", "00956888", 20]])
| .after_unload |= map(.reads += engine_reads + [["value-size-dispatch", "00d50084", 4], ["value-working-cells", "00956888", 20]])
