#!/usr/bin/env bash
set -euo pipefail

export JQ="$PWD/$1"
apply="$PWD/$2"
cd "$TEST_TMPDIR"

fresh_ledger() {
  printf '%s\n' \
    '{"id":"E-A","claim":"caf\u00e9 kept as written"}' \
    '{"id":"E-B","claim":"old"}' \
    '{"id":"E-C","claim":"gone"}' > ledger.jsonl
}

fresh_ledger
printf '%s' '{"remove":["E-C"],"replace":[{"id":"E-B","claim":"new"}],"add":[{"id":"E-D","claim":"added"}]}' > transform.json
"$apply" ledger.jsonl transform.json
printf '%s\n' \
  '{"id":"E-A","claim":"caf\u00e9 kept as written"}' \
  '{"id":"E-B","claim":"new"}' \
  '{"id":"E-D","claim":"added"}' > expected.jsonl
cmp ledger.jsonl expected.jsonl

for refused in \
  '{"remove":["E-X"]}' \
  '{"replace":[{"id":"E-X","claim":"x"}]}' \
  '{"add":[{"id":"E-A","claim":"x"}]}' \
  '{"remove":["E-B"],"replace":[{"id":"E-B","claim":"x"}]}' \
  '{"remove":["E-B","E-B"]}' \
  '{"add":[{"claim":"no id"}]}' \
  '{"delete":["E-B"]}'; do
  fresh_ledger
  printf '%s' "$refused" > transform.json
  if "$apply" ledger.jsonl transform.json 2> /dev/null; then
    echo "Transform incorrectly accepted: $refused" >&2
    exit 1
  fi
  printf '%s\n' \
    '{"id":"E-A","claim":"caf\u00e9 kept as written"}' \
    '{"id":"E-B","claim":"old"}' \
    '{"id":"E-C","claim":"gone"}' > unchanged.jsonl
  cmp ledger.jsonl unchanged.jsonl
done

printf '%s\n' '{"id":"E-A"}' '{"id":"E-A"}' > ledger.jsonl
printf '%s' '{"remove":[]}' > transform.json
if "$apply" ledger.jsonl transform.json 2> /dev/null; then
  echo 'Ledger with a duplicate id incorrectly accepted' >&2
  exit 1
fi
