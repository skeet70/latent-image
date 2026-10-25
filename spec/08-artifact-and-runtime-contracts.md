# Artifact and runtime contracts

This document defines the artifact and runtime contracts used by [the shared interfaces](02-project-and-environment.md#7-shared-contracts) and [execution phases](03-phases-and-gates.md#8-execution-phases-and-gates). Schema version 1 is the interchange version; target-specific values require evidence.

## Artifact identity and provenance

Hash exact file bytes with SHA-256 where provenance matters: firmware inputs, captures, payload binaries and stock fixtures. Store an artifact's path and digest in the manifest that consumes it; a manifest does not embed its own digest.

Represent addresses and register bit patterns as hexadecimal strings. Represent timestamps/counter ticks as decimal strings so consumers preserve full integer precision. Lengths and allocation sizes remain bounded integers; check addition, address-width overflow, host allocation limits, and region containment before reading or mapping. Never map an entire unknown address space to make a fixture pass.

Keep an artifact inventory with size, hash, origin, collection command/tool version, coverage, and privacy class. Body-specific data and expensive captures need two durable copies; record a successful checksum verification of both. An ignored local folder alone is not a backup. Firmware containers, raw storage, configuration mirrors, calibration, and identity stay distinct.

Manifests and JSON schemas are in `manifests/` and `schemas/`. Unknown values use JSON `null`; do not substitute zero or a plausible address. Reject unknown schema versions and unsupported operations before device access.

## Device selection

A camera command selects exactly one connected `04cb:ff80` device with the supported descriptor profile and refuses otherwise. VID/PID is only a selector; model identity comes from the product query or the read plan's oracle window. Only one Fujifilm body is in use; bind identity more tightly if a second body appears.

## Transport and cleanup

Do not automatically retry a command that might have changed state when its reply is lost. Test short replies, zero progress, partial transfers, cancellation and timeouts through the injected transport. Timeouts bound both individual transfers and the whole command.

Raw packets go into the run's private output directory. Log outgoing bytes before dispatch and response bytes/status with monotonic host timestamps, so a planned action can be distinguished from one that may have reached the camera. Process exit releases host resources; it does not show the camera returned to normal. The owner's power-cycle and shot after the run does that. USB direction does not determine operation class.

For future temporary changes (debug flags, RAM payloads) use explicit observe-original, apply, verify, restore and verify-restoration stages, and record incomplete cleanup in the journal.

## Clocks, observations, and controller actions

Every trace header declares its schema version, body/firmware/lens and build hashes, scene/session/region, position units, and each clock's source, tick rate, origin, wrap width, synchronization method, and uncertainty. Acquisition, lens dispatch/completion, statistic delivery, and external video may use different clocks. A missing synchronization remains missing; do not subtract unrelated timestamps to invent latency.

Each observation carries sequence and command IDs, acquisition/delivery timestamps with clock IDs, the focus command in effect during acquisition, reported lens position/completion if available, validity and invalid reason, sharpness/region, exposure/gain/aperture, and cancellation/mode state. Record dropped frames and ring-buffer overflow. A sample belongs to the lens state at sensor acquisition, even if delivery occurs after another move. Synthetic, replayed, and hardware events carry explicit origin.

Actions are `hold`, `move_to`, `move_by`, `finish`, `fail`, or `hand_back_to_stock`. Each carries its triggering observation/command ID, units, bounded target/delta, reason, and an expected next event or timeout. Admit an action only within measured lens bounds and the validated completion semantics. A confidence field is optional and must describe a calibrated meaning.

Keep this contract small enough to mirror in a bounded state/step interface. Define integer widths, units, arithmetic/overflow, structure layout where relevant, and serialization explicitly. Do not serialize raw host structs or require Haskell on the camera. Haskell can orchestrate files/stdio around the controller and native runner; use a narrow FFI only if a measured requirement justifies it. Implementation languages follow [current principles](../docs/principles.md#implementation-and-build).

### Camera snapshots and live experiment sessions

The future camera-observation producer must publish a versioned snapshot contract before its consumers ship. Reuse the observation/action identities above; do not label raw bytes as a validated frame or add hardware origins to the synthetic-only simulator contract.

- Bind each snapshot to session epoch, frame/sample identity, source domain, format, dimensions/stride/crop, payload extent, sensor/region/processing configuration, exposure/gain and acquisition-time lens command. Unknown acquisition time, position or completion stays explicitly unknown.
- Establish producer completion, ownership and cache/alias visibility. Reading a header twice or obtaining a checksum does not prove a DMA buffer remained immutable during extraction.
- Prefer a completed stock buffer with a justified lifetime; otherwise copy a bounded crop/statistic record into owned export storage. Publish it only after completion, retain it until release or a bounded expiry, and reject stale epochs or released snapshots.
- Never wait for USB inside an acquisition callback or stall the sensor for a slow consumer. Bound copying, storage and callback time; record overflow/drop/contention and incomplete samples.
- Validate active extents against evidenced memory backing and ownership. Firmware pool tables and emulator RAM capacity do not justify widening physical read bounds.
- Permit one active focus controller. Camera-side and host-side checks enforce the reviewed mode, command family, position/rate/travel bounds, finite attempt/session budgets and cancellation. Expose no arbitrary memory/MMIO forwarding from an emulator.
- A host loss or uncertain state-changing reply stops new actions without retry. Define safe completion/cleanup of already accepted work; a timeout cannot guarantee recovery from arbitrary privileged-code corruption.
- Reconnect, reset or emulator checkpoint restore invalidates the live session and pending actions. Re-establish identity and observed state before a separately admitted continuation.
- Offline source providers cannot enumerate or open a camera. Hardware transport exists only inside explicit reviewed `//transport:camera` operations.


## Native-case and payload requirements

A native case names exact input hashes/offsets, evidenced CPU family/model/mode/endianness, initial registers/flags, stack/global bytes, mapped regions/permissions, transitive calls, explicit stub contracts, and nonzero instruction/wall-time budgets. Expected results include registers, allowed memory differences, exit reason, and stub/event logs. An independent known result is required for a stock case; a self-generated expected value is insufficient.

The runner checks every Unicorn return code, refuses unsupported CPU features, traps unknown accesses, and preserves reproducible failures. Account for mapping alignment without granting unintended page permissions. Stock ABI/exception/coprocessor/cache/DMA/scheduling behavior must be modeled or excluded with scope. Audit upstream example scripts before reusing their contracts under [the implementation policy](../docs/principles.md#implementation-and-build).

A payload manifest additionally names the exact firmware port, compiler/toolchain hash and flags, linker map/disassembly, expected original and replacement bytes at every patch site, owned scratch memory and lifetime, exact write set and external calls, entry/return/stack/register contracts, synchronization, resource budgets, activation/removal, and fallback. Observe unchanged stock bytes immediately before patching. Native arithmetic success alone does not validate caller selection, cancellation, instruction visibility, or runtime integration.

The [native-case schema](../schemas/native-case.schema.json) and [AF observation/action schema](../schemas/af-observation-action.schema.json) define the machine contracts. Payload manifests must satisfy the requirements above; agree any additional schema before implementing competing consumers. The engine-initialization command does not consume native cases or establish target behavior.

## Release bundles

G6 includes an untethered-use check: after loading, disconnect the host through the validated path and confirm that inference/control remain on-camera, overflow or absent logging is handled, and unsupported modes/cancellation use coherent stock hand-back. Record cold-start loss and reactivation of a RAM implementation. G7's operator guide explains activation, supported settings, removal, ordinary host loss, payload crash, and demonstrated recovery limits. Persistent installation is still a separate decision.
