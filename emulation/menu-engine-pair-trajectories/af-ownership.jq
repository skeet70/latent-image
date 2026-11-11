if ([.before_unload[] | select(.phase == "value-move")] | length) != 1
  or ([.before_unload[] | select(.phase == "value-accept-left")] | length) != 1
then error("AF ownership requires the original value-move and value-accept-left phases")
else .before_unload |= map(
  if .phase == "value-move" then
    if .steps != [["down", 1, 246, 1]] then error("AF ownership DOWN input differs")
    else .af_ownership = "arm" end
  elif .phase == "value-accept-left" then
    if .steps != [["left", 1, 5, 3]] then error("AF ownership LEFT input differs")
    else .af_ownership = "end" end
  else . end)
end
