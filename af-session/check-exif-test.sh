#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
set -euo pipefail
checker=$1
jq_bin=$2
work=$(mktemp -d "${TEST_TMPDIR:-/tmp}/check-exif.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir "$work/frames"
printf 'synthetic frame placeholder\n' > "$work/frames/reference.JPG"
cat > "$work/exiftool" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[[ ${EXIF_FAIL:-0} == 0 ]] || exit 1
cat "$EXIF_JSON"
SH
chmod +x "$work/exiftool"
export EXIF_JSON="$work/exif.json"
cat > "$work/pilot-settings.json" <<'JSON'
{"gain_iso":200,"aperture":4.5,"shutter_time_s":0.01,"focal_length_mm":28,"camera_firmware":"1.50","exposure_mode":"M","lens_model":"Viltrox AF 28mm F4.5","processing":"FINE+RAW, PROVIA/STANDARD, 16:9, rest default","af_region":"one up and one right of centre, 100% size (EXIF FocusPixel 3015 1060)"}
JSON
cat > "$work/matching.json" <<'JSON'
[{"ISO":200,"FNumber":4.5,"ShutterSpeed":0.01,"FilmMode":0,"ImageSize":"4896 2760","FocusPixel":"3015 1060","FocusWarning":0,"Software":"Digital Camera X-M1 Ver1.50","LensModel":"AF 28/4.5 XF   ","ExposureMode":1,"FocalLength":28}]
JSON
cp "$work/matching.json" "$EXIF_JSON"
bash "$checker" "$jq_bin" "$work/exiftool" "$work/frames/reference.JPG"
bash "$checker" "$jq_bin" "$work/exiftool" "$work/frames/reference.JPG" "$work/pilot-settings.json"
reject() {
  if bash "$checker" "$jq_bin" "$work/exiftool" "$work/frames/reference.JPG" "$work/pilot-settings.json"; then
    echo "accepted invalid photographic comparison: $1" >&2
    exit 1
  fi
}
for change in '.[0].ISO = 400' '.[0].FNumber = 5.6' '.[0].ShutterSpeed = 0.02' \
  '.[0].FilmMode = 1' '.[0].ImageSize = "1 1"' '.[0].FocusPixel = "0 0"' \
  '.[0].FocusWarning = 1' '.[0].Software = "Digital Camera X-M1 Ver1.00"' \
  '.[0].LensModel = "other"' '.[0].ExposureMode = 0' '.[0].FocalLength = 35' \
  'del(.[0].ISO)' '.[0].Error = "unreadable"' '. + .' '. = []'; do
  "$jq_bin" "$change" "$work/matching.json" > "$EXIF_JSON"
  reject "$change"
done
cp "$work/matching.json" "$EXIF_JSON"
EXIF_FAIL=1 reject 'extractor failure'
printf 'not JSON\n' > "$EXIF_JSON"
reject 'malformed metadata'
cp "$work/matching.json" "$EXIF_JSON"
"$jq_bin" '.lens_model = "other"' "$work/pilot-settings.json" > "$work/unsupported.json"
mv "$work/unsupported.json" "$work/pilot-settings.json"
reject 'unsupported lens profile'
