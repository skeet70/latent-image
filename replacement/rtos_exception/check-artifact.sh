#!/bin/sh
set -eu

symbols_a=$1
symbols_b=$2
disasm_a=$3
disasm_b=$4

address() {
  awk -v name="$2" '$4 == name { print "0x" $1; found = 1 } END { if (!found) exit 1 }' "$1"
}

check() {
  actual=$(address "$1" "$2")
  if [ "$actual" != "$3" ]; then
    echo "$2 is $actual, expected $3" >&2
    exit 1
  fi
}

check "$symbols_a" xm1_exception_undefined 0x00040100
check "$symbols_a" xm1_exception_prefetch 0x00040128
check "$symbols_a" xm1_exception_data 0x00040148
check "$symbols_a" xm1_exception_reserved 0x00040168
check "$symbols_a" xm1_exception_fiq 0x00040184
check "$symbols_a" xm1_exception_svc 0x000401b4
check "$symbols_a" xm1_exception_svc_diag 0x000401e4
check "$symbols_a" xm1_exception_irq 0x00040204
check "$symbols_a" xm1_exception_dispatcher 0x00040cac
check "$symbols_a" xm1_exception_ack_wrapper 0x00040d50
check "$symbols_a" xm1_exception_eoi_wrapper 0x00040d60
check "$symbols_a" xm1_exception_fiq_hook 0x00040d70
check "$symbols_a" xm1_exception_ack 0x000430a8
check "$symbols_a" xm1_exception_vector_slot 0x00043810

check "$symbols_b" xm1_exception_undefined 0x000d0100
check "$symbols_b" xm1_exception_prefetch 0x000d0128
check "$symbols_b" xm1_exception_data 0x000d0148
check "$symbols_b" xm1_exception_reserved 0x000d0168
check "$symbols_b" xm1_exception_fiq 0x000d0184
check "$symbols_b" xm1_exception_svc 0x000d01b4
check "$symbols_b" xm1_exception_svc_diag 0x000d01e4
check "$symbols_b" xm1_exception_irq 0x000d0204
check "$symbols_b" xm1_exception_dispatcher 0x000d0cac
check "$symbols_b" xm1_exception_ack_wrapper 0x000d0d50
check "$symbols_b" xm1_exception_eoi_wrapper 0x000d0d60
check "$symbols_b" xm1_exception_fiq_hook 0x000d0d70
check "$symbols_b" xm1_exception_ack 0x000d30a8
check "$symbols_b" xm1_exception_vector_slot 0x000d3810

for name in xm1_exception_lock_acquire xm1_exception_eoi xm1_exception_exit_tail xm1_exception_fault_record; do
  for symbols in "$symbols_a" "$symbols_b"; do
    address "$symbols" "$name" >/dev/null
  done
done

grep -q '40204:.*mov.*sp, #211' "$disasm_a"
grep -q 'd0204:.*mov.*sp, #211' "$disasm_b"
grep -q '40cac:.*push.*r4, r5, r11, lr' "$disasm_a"
grep -q 'd0cac:.*push.*r4, r5, r11, lr' "$disasm_b"
grep -q '40318:.*b.*0x4375c' "$disasm_a"
grep -q 'd0318:.*b.*0xd375c' "$disasm_b"
grep -q '43810:.*bx.*lr' "$disasm_a"
grep -q 'd3810:.*bx.*lr' "$disasm_b"
grep -q '40360:.*msr.*SPSR_fsxc, r1' "$disasm_a"
grep -q 'd0360:.*msr.*SPSR_fsxc, r1' "$disasm_b"

echo "exception artifact entries and first words match their stock extents"
