include "engine";
$config[0] as $c
| $receipt[0] as $r
| if $r.key != $c.key or $r.key_reply.menu == null then error("Fn receipt does not match configured key") else . end
| ($r.key_reply.menu | [.menu_id,.setting_id,.cursor]) as $landing
| (if $name == "fn" and $c.path == [] and $c.key == "fn" then []
   elif $name == "fn-down" and $c.path == ["fn"] and $c.key == "down" then [["fn",99,321,17]]
   elif $name == "fn-auto-open" and $c.path == ["fn","down","down"] and $c.key == "right" then
     [["fn",99,321,17],["down",99,321,18],["down",99,321,20]]
   else error("unqualified Fn oracle path") end) as $path
| def reads: [["dispatcher-first-word","00bfb028",4],["resident","runtime:rb_resident",1024],["live-settings","0096cc64",450]] + engine_reads;
  def segment($phase;$capture;$steps): {phase:$phase,capture:$capture,steps:$steps,reads:reads,
    read_preservations:[{read:"resident",phase:"package-menu-prefix",source_read:"resident"}]};
  {
    schema:1,
    entry:"Joint menu-engine/render pair, \($role) role, Fn oracle \($name).",
    source_bindings:[
      {source:"emulation/menu-oracle-\($name).json",expectation:"The configured path and key are unchanged."},
      {source:$receipt_source,expectation:"The observed key lands on \($landing)."},
      {source:"reports/owned-menu-engine-increment-5.md:21,29,58-69",expectation:"ISO opens at cursor 17; DOWN visits 18 then20 because cursor19 is hidden. RIGHT on20 opens AUTO ISO. BACK closes both ISO and its Fn AUTO child, rather than popping the child; the closed menu tuple is retained."},
      {source:"private-data/scratch/ControllerStrategy/menu-oracle-open/menu-observe.json and inc3/close/menu-observe.json",expectation:"The modal sample opens and closes the shooting menu before the Fn path."}
    ],
    trace_sites:engine_sites($role),
    before_unload:[
      {phase:"package-menu-prefix",capture:"oracle-live-view",steps:[],reads:reads},
      segment("modal-sample";"modal-sample";($sample | presented_handoff))
    ] + (if $path|length > 0 then [segment("oracle-path";"oracle-path";$path)] else [] end)
      + [segment("oracle-key";"oracle-key";[[$c.key] + $landing]),
         segment("close-menu";"close-menu";[["back"] + $landing])],
    after_unload:[]
  }
| render_trajectory
