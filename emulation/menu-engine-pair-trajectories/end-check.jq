$graph[0].graph as $g
| ($g.edges | map(select(.to != null) | {key:"\(.from):\(.key)",value:.to}) | from_entries) as $edge
| ($g.nodes | map({key:(.id|tostring),value:.}) | from_entries) as $node
| def menu($id): $node[$id|tostring].menu | [.menu_id,.setting_id,.cursor];
  def state($id): $node[$id|tostring].states[296:300];
  if $name | startswith("oracle-fn") then
    if (["oracle-fn","oracle-fn-down","oracle-fn-auto-open"] | index($name)) == null
      or $t[0].before_unload[-1].phase != "close-menu"
      or $t[0].before_unload[-1].steps != [["back"] + $t[0].before_unload[-2].steps[-1][1:4] + [{completion:"presented-frame"}]]
    then error("Fn trajectory does not end with its qualified direct close")
    else {at:0,state:"0400",proof:"reports/owned-menu-engine-increment-5.md:69, direct ISO/Fn AUTO close"} end
  else
    [$t[0].before_unload[] | select(.phase != "package-menu-prefix" and .phase != "modal-sample" and .phase != "render-baseline") | .steps[]] as $all
    | [range(0;$all|length) as $i | $all[$i]
       | if .[0:4] == ["up",1,5,33] or (.[0:4] == ["up",1,5,32] and $i > 0 and $all[$i-1][0:4] == ["up",1,5,33]) then empty else .[0:4] end] as $steps
    | reduce $steps[] as $s ({at:0,prev:null,ok:true};
        if .ok|not then . else ($edge["\(.at):\($s[0])"]) as $to
          | if $to != null then . + {at:$to,prev:.at}
            elif state(.at) == "0400" then . + {ok:false,missing:$s}
            elif $s[1:] == menu(.at) then .
            elif .prev != null and $s[1:] == menu(.prev) then . + {at:.prev,prev:.at}
            else . + {ok:false,missing:$s} end end)
    | if .ok and state(.at) == "0400" then {at:.at,state:state(.at)} else error("before-unload does not end at controller74/state4: \(.)") end
  end
