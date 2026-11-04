.graph as $g
| ($g.edges | map(select(.to != null) | {key: "\(.from):\(.key)", value: .to}) | from_entries) as $edge
| ($g.nodes | map({key: (.id | tostring), value: .}) | from_entries) as $node
| def menu($id): $node[$id | tostring].menu | [.menu_id, .setting_id, .cursor];
  def walk($from; $keys): reduce $keys[] as $k ({at: $from, steps: []};
      if .at == null then . else
        ($edge["\(.at):\($k)"]) as $to
        | {at: $to, steps: (.steps + [[$k, (if $to == null then null else menu($to) end)]])}
      end);
  [15,26,36,163,211,214,239,246,250,260,270,271,272,273,297,312,314,320,450,488] as $pickers
| [ $pickers[] as $p
    | ([$g.nodes[] | select(.menu.menu_id == 1 and .menu.setting_id == $p)] | sort_by(.depth, .id) | .[0]) as $picker
    | walk(0; $picker.path[0:-1]) as $route
    | walk($route.at; ["ok","down","up","right","left","ok","ok","ok","back"]) as $keys
    | walk($keys.at; ["ok","back"]) as $after
    | walk($picker.id; ["right","left","ok","ok","ok","back"]) as $from_picker
    | walk($from_picker.at; ["ok","back"]) as $from_picker_after
    | {setting: $p, picker: $picker.id, route: $route.steps, keys: $keys.steps, after: $after.steps,
       from_picker: $from_picker.steps, from_picker_after: $from_picker_after.steps,
       complete: ([$route.steps[], $keys.steps[], $after.steps[]] | all(.[1] != null))} ]
