# Decodes the EMB2 demonstration record (replacement/embassy_platform/src/platform.rs `Demo`)
# from a lowercase hex read.
def bytes: [range(0; length; 2) as $i | .[$i:$i+2] | explode | map(if . >= 97 then . - 87 else . - 48 end) | .[0] * 16 + .[1]];
def u32($b; $o): $b[$o] + $b[$o + 1] * 256 + $b[$o + 2] * 65536 + $b[$o + 3] * 16777216;
def i32($b; $o): u32($b; $o) as $v | if $v >= 2147483648 then $v - 4294967296 else $v end;
def status($b; $o): [i32($b; $o), i32($b; $o + 4), i32($b; $o + 8)] | map(if . == -2147483648 then "not-called" else . end);
def domain($b; $o): {
  entered: u32($b; $o), direction_measured: u32($b; $o + 4), ticks_only: u32($b; $o + 8),
  ticker_iterations: u32($b; $o + 12), first_tick: u32($b; $o + 16), last_tick: u32($b; $o + 20),
  early_kernel_wakes: u32($b; $o + 24), late_max: u32($b; $o + 28), late_over_two_ticks: u32($b; $o + 32),
  sleeps_woken: u32($b; $o + 36), sleeps_timed_out: u32($b; $o + 40), sleeps_other: u32($b; $o + 44),
  last_sleep_other: i32($b; $o + 48), pender_task_calls: u32($b; $o + 52), pender_handler_calls: u32($b; $o + 56),
  pender_unavailable: u32($b; $o + 60), pender_errors: u32($b; $o + 64), last_pender_error: i32($b; $o + 68),
  doorbell_received: u32($b; $o + 72), doorbell_latency_max: u32($b; $o + 76),
  clock_regressions: u32($b; $o + 80), clock_regression_max: u32($b; $o + 84),
  stack_base: u32($b; $o + 88), stack_bytes: u32($b; $o + 92)
};
def demo: bytes as $b | {
  magic: ([$b[0:4][] | [.] | implode] | add), start_calls: u32($b; 4), start_skipped: u32($b; 8),
  started: u32($b; 12), panics: u32($b; 16), start_status: {A: status($b; 20), B: status($b; 32)},
  doorbell_sent: u32($b; 44), doorbell_seq: u32($b; 48),
  domains: {A: domain($b; 60), B: domain($b; 156)}
};
