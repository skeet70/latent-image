#!/usr/bin/env bash
set -euo pipefail

export JQ="$PWD/$1"
apply="$PWD/$2"
cd "$TEST_TMPDIR"

printf '%s\n' \
  '{"id":"x-a","claim":"caf\u00e9 kept as written"}' \
  '{"id":"x-b","claim":"old"}' \
  '{"id":"x-c","claim":"gone"}' > original.jsonl

accepted() {
  cp original.jsonl ledger.jsonl
  printf '%s' "$1" > transform.json
  "$apply" ledger.jsonl transform.json
  cmp ledger.jsonl expected.jsonl
}

printf '%s\n' \
  '{"id":"x-a","claim":"caf\u00e9 kept as written"}' \
  '{"id":"x-b","claim":"new"}' \
  '{"id":"x-d","claim":"added"}' > expected.jsonl
accepted '{"remove":["x-c"],"replace":[{"id":"x-b","claim":"new"}],"add":[{"id":"x-d","claim":"added"}]}'

{ cat original.jsonl; printf '%s\n' '{"id":"x-d"}'; } > expected.jsonl
accepted '{"add":[{"id":"x-d"}]}'

cp original.jsonl expected.jsonl
accepted '{}'

for refused in \
  '{"remove":["x-z"]}' \
  '{"replace":[{"id":"x-z","claim":"z"}]}' \
  '{"add":[{"id":"x-a","claim":"z"}]}' \
  '{"remove":["x-b"],"replace":[{"id":"x-b","claim":"z"}]}' \
  '{"remove":["x-b","x-b"]}' \
  '{"add":[{"claim":"no id"}]}' \
  '{"add":["x-d"]}' \
  '{"remove":"x-b"}' \
  '{"remove":{"k":"x-b"}}' \
  '{"delete":["x-b"]}' \
  '{"add":[{"id":"x-d"}]} {"add":[{"id":"x-d"}]}' \
  '[]'; do
  cp original.jsonl ledger.jsonl
  printf '%s' "$refused" > transform.json
  if "$apply" ledger.jsonl transform.json 2> /dev/null; then
    echo "Transform incorrectly accepted: $refused" >&2
    exit 1
  fi
  cmp ledger.jsonl original.jsonl
done

printf '%s' '{}' > transform.json
for bad_ledger in \
  '{"id":"x-a"}\n{"id":"x-a"}\n' \
  '{"id":"x-a"}\n\n{"id":"x-b"}\n' \
  '{"id":"x-a"}{"id":"x-b"}\n' \
  '{"id":"x-a"}'; do
  printf "$bad_ledger" > ledger.jsonl
  cp ledger.jsonl before.jsonl
  if "$apply" ledger.jsonl transform.json 2> /dev/null; then
    echo "Ledger incorrectly accepted: $bad_ledger" >&2
    exit 1
  fi
  cmp ledger.jsonl before.jsonl
done
