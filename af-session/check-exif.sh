#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
set -euo pipefail
umask 077

jq_bin=$1
exiftool_bin=$2
shift 2
frame=${1:-}
[[ $# -ge 1 && $# -le 2 && -f $frame && -r $frame ]] || { echo 'usage: check-exif FRAME.JPG [PILOT_SETTINGS_JSON]' >&2; exit 2; }
pinned=${2:-}
if [[ -z $pinned ]]; then
  pinned=$(cd "$(dirname "$frame")/.." && pwd)/pilot-settings.json
fi
[[ -f $pinned && -r $pinned ]] || { echo "pinned settings not readable: $pinned" >&2; exit 2; }

# Display strings cover the S01-S03 X-M1/Viltrox photographic profile.
"$jq_bin" -e '
  type == "object" and
  (.gain_iso | type == "number" and . > 0) and
  (.aperture | type == "number" and . > 0) and
  (.shutter_time_s | type == "number" and . > 0) and
  (.focal_length_mm | type == "number" and . > 0) and
  (.camera_firmware | type == "string" and length > 0) and
  .exposure_mode == "M" and .lens_model == "Viltrox AF 28mm F4.5" and
  .processing == "FINE+RAW, PROVIA/STANDARD, 16:9, rest default" and
  .af_region == "one up and one right of centre, 100% size (EXIF FocusPixel 3015 1060)"
' "$pinned" >/dev/null || { echo 'FAIL invalid or unsupported pinned profile' >&2; exit 1; }

exif=$("$exiftool_bin" -j -n -ISO -ShutterSpeed -FNumber -FilmMode -ImageSize \
  -FocusPixel -FocusWarning -Software -LensModel -ExposureMode -FocalLength "$frame") \
  || { echo 'FAIL EXIF extraction failed' >&2; exit 1; }
checks=$("$jq_bin" -er --slurpfile pinned "$pinned" '
  if length != 1 or (.[0].Error != null) then error("expected one readable frame") else .[0] end
  | . as $e | $pinned[0] as $p
  | [
      ["ISO", $e.ISO, $p.gain_iso, ($e.ISO == $p.gain_iso)],
      ["FNumber", $e.FNumber, $p.aperture, ($e.FNumber == $p.aperture)],
      ["ShutterSpeed seconds", $e.ShutterSpeed, $p.shutter_time_s,
        (if ($e.ShutterSpeed | type) == "number" then
          ($e.ShutterSpeed > $p.shutter_time_s * 0.98 and $e.ShutterSpeed < $p.shutter_time_s * 1.02)
         else false end)],
      ["FilmMode", $e.FilmMode, 0, ($e.FilmMode == 0)],
      ["ImageSize", $e.ImageSize, "4896 2760", ($e.ImageSize == "4896 2760")],
      ["FocusPixel", $e.FocusPixel, "3015 1060", ($e.FocusPixel == "3015 1060")],
      ["FocusWarning", $e.FocusWarning, 0, ($e.FocusWarning == 0)],
      ["Software", $e.Software, ("Digital Camera X-M1 Ver" + $p.camera_firmware),
        ($e.Software == "Digital Camera X-M1 Ver" + $p.camera_firmware)],
      ["LensModel", $e.LensModel, "AF 28/4.5 XF",
        (($e.LensModel // "" | sub(" +$"; "")) == "AF 28/4.5 XF")],
      ["ExposureMode", $e.ExposureMode, 1, ($e.ExposureMode == 1)],
      ["FocalLength", $e.FocalLength, $p.focal_length_mm, ($e.FocalLength == $p.focal_length_mm)]
    ]
  | .[] | if .[3] then "PASS \(.[0]): \(.[1])"
      else "FAIL \(.[0]): observed \(.[1]), pinned \(.[2])" end
' <<<"$exif") || { echo 'FAIL malformed EXIF or pinned settings' >&2; exit 1; }
printf '%s\n' "$checks"
printf '%s\n' 'NOTE lens firmware, AF mode/size, AF assist, FINE+RAW and remaining defaults require camera readback; EXIF checks do not verify them.'
if [[ $checks == *FAIL* ]]; then
  printf '\ncheck-exif %s: mismatch; not a comparison point.\n' "$(basename "$frame")"
  exit 1
fi
printf '\ncheck-exif %s: all EXIF-covered fields match.\n' "$(basename "$frame")"
