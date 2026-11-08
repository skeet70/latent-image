def renamed($prefix):
  .phase = ($prefix + .phase)
  | if .capture != null then .capture = ($prefix + .capture) else . end
  | if has("read_preservations") then
      .read_preservations |= map(.phase = ($prefix + .phase))
    else . end;

def probe_site($address; $label): [$address, $label, {lr:true}];

{
  schema:1,
  entry:"Stock-only draw-route probe: picker-15 opens/closes the shooting menu, then Fn ISO opens the second-level AUTO ISO menu. One unload follows both routes; no after-unload keys.",
  source_bindings:($picker.source_bindings + $oracle.source_bindings + [
    {source:"emulation/menu-engine-pair-trajectories/draw-route-probe.jq",expectation:"The picker route provides the shared modal open/close sample. The standalone Fn modal sample is omitted because the picker leaves its remembered shooting-menu cursor at 32."},
    {source:"private-data/analysis/x-m1-1.50/filmsim/plain.dis:8844-8845",expectation:"At 0x01108a14, add r0,r6,#0x4000 precedes uxth at 0x01108a18. The companion hit recovers r6 as (r0-0x4000) modulo 2^32."}
  ]),
  trace_sites:((($picker.trace_sites + $oracle.trace_sites)|unique) + [
    probe_site("011089d8";"probe-selected-handler-blx-r3"),
    probe_site("0115815c";"probe-root-drawer"),
    probe_site("01158188";"probe-choice-root-wrapper"),
    probe_site("011581e0";"probe-tree-root-wrapper"),
    probe_site("01166200";"probe-root-prepare"),
    probe_site("011662a0";"probe-root-pane-drawer"),
    probe_site("01164e1c";"probe-tree-entry"),
    probe_site("01163f2c";"probe-picker-entry"),
    probe_site("01108a14";"probe-uilib-event-before-add"),
    probe_site("01108a18";"probe-uilib-event-r6-via-r0")
  ]),
  before_unload:([$picker.before_unload[]|renamed("probe-picker-")]
    + [$oracle.before_unload[]|select(.phase != "modal-sample")|renamed("probe-fn-")]),
  after_unload:[]
}
