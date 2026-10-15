# Recovery research

The [public recovery kit](../recovery/README.md) documents supported repairs, reusable host card preparation and firmware triage/extraction. It does not turn an unclassified failure or a valid DAT into an accepted persistent recovery operation.

The recorder rev-2 failed-boot incident and recovery-adjacent checks are complete. Exact v5 physical rollback reports successful writer return and complete RC0 readback at unchanged block 14, followed by owner-reported ordinary startup after power removal, photography, normal-card operation/playback and retained film simulations/settings, including C-slot color +2 (E-RECOVERY-FRAGMENTED-V5-STATUS20-150, E-RECOVERY-V5-NORMAL-CARD-PLAYBACK-150, E-RECOVERY-V5-RETENTION-150). [The recovery findings](../reports/sd-boot-card-entry.md) retain the version-specific scope. [Recovery status](../reports/recovery-status.md) records R3 demonstrated 2026-10-13 and R4 formally unclaimed. Preserve the successful card; these results do not authorize another recovery write or firmware reinstall.

## 10. Recovery work package in detail

**Owner C's objective:** determine exactly which failures can be recovered and establish the least intrusive useful recovery route before firmware modification.

### 10.1 Evidence levels

| Level | Evidence | Claim allowed |
| --- | --- | --- |
| R0 | Only documentation or sibling-camera findings. | Recovery is a lead. |
| R1 | X-M1 ROM/storage boot paths mapped; entry prerequisites understood from local artifacts. | A candidate recovery design exists for this version/body. |
| R2 | X-M1 executes a bounded SD/RAM diagnostic through an early path whose independence from the application is established. | An independent execution foothold is demonstrated. |
| R3 | That path reads relevant storage and identifies a valid stock update/restore target; a dry run validates boundaries and prerequisites. | A recovery loader can reach the storage it would need; writes remain unproved. |
| R4 | A specifically defined failure has been repaired through this route, with verified data restoration and normal operation. | Recovery is demonstrated for that failure class. |

R1 is accepted on 2026-10-08. R2 is demonstrated on 2026-10-12 by exact-card execution of the bounded startup-restored marker on the failed body (E-SDBOOT-BLINK-WARM-OBSERVED-150). These milestones and the completed v5 rollback do not change the formal R4 claim without a recorded gate decision.

R2 does not imply R4. A recovery path that depends on intact boot configuration or readable calibration cannot be advertised as recovering corruption of those same dependencies.

### 10.2 Ordered tasks

**C1 — Boot dependency map.** Trace reset through ROM, internal storage initialization, optional SD path, image validation, and application entry. Identify how buttons/straps/settings select a path. Record what memory and device state each stage assumes. Assess the user's body revision where observable.

**C2 — Configuration preservation.** Separate model-wide firmware from unique serial/calibration data and from mutable settings/counters. Determine which persistent regions can be read, which are only mirrored in RAM, and what cannot yet be backed up. Check repeated reads of static regions and use an independent durable copy.

**C3 — Minimal early payload.** Construct a model-specific, bounded diagnostic. Prefer an understood existing output primitive or verified memory return path. A GPIO indicator is acceptable only after mapping the relevant register/pin for X-M1. Review all initialization writes, not just the C function body. No flash erase/write operations in this stage.

**C4 — Storage read and recovery planning.** Identify whether the hardware uses NAND/eMMC/another format and how ECC, spare/OOB data, bad blocks, geometry, boot headers, and calibration partitions are handled. A firmware DAT is an update container, not automatically a chip image. Never write it raw to storage.

**C5 — Reuse the native updater where justified.** Investigate launching the correct native updater from an independently loaded environment. This may reuse storage logic but creates dependencies on initialization, tasking, configuration, and target-selection code. Map and test those dependencies. Do not presume copying the updater function is enough.

**C6 — Host recovery model.** Test the parser, partition/range selection, validation, interrupted operations, wrong model/version, truncated images, bad checksums, and preservation of excluded regions using files. Prefer independent format checks where possible. This resolves specific recovery hazards; it does not validate physical writes.

**C7 — Hardware validation proposal.** Produce an exact sequence for a spare identical body or already-bricked compatible board if available. Define the corruption/failure being tested, required backups, what must survive, what will be written, and success measurements. Do not acquire hardware or intentionally corrupt the primary body without user authorization.

### 10.3 Required failure classification

| Failure | Candidate way back | Evidence needed |
| --- | --- | --- |
| Host process or USB session stalls | End session or ordinary restart if only safe transient operations were active | Known operation state; no internal storage write in progress. |
| RAM payload crashes | Cold start should discard ordinary volatile changes | Payload access audit and successful return-to-stock checks; acknowledge collateral-write possibilities. |
| Application firmware is invalid but ROM entry survives | Independent SD recovery | R4 for that class before claiming demonstrated recovery. A persistent experiment without that proof requires explicit informed acceptance of the stated residual risk under the recorded R4 practice decision. |
| Calibration/EEPROM is corrupted | Correct body-specific restore through an independent route | Genuine persistent backup and validated writer; runtime cfgdata alone is insufficient. |
| Boot metadata/controller initialization is damaged | Depends on ROM and board design | Specific analysis; may require external hardware recovery. |
| Lens firmware or physical hardware is damaged | Separate recovery/repair | Body firmware recovery does not cover it. Exclude lens flashing and electrical tuning from this project. |

### 10.4 Incident handling

Stop new requests, keep the run's output directory, and determine whether a persistent operation could be in progress. The generic upstream timeout message suggesting a battery removal must not be copied into every failure path: interruption during an actual storage write can worsen the situation. For transient-only activity, exit jig mode and power-cycle normally. If the state is unknown, work it out with the owner rather than cycling increasingly invasive commands.

Recovery research starts early. Do not rely on a repair capability beyond its demonstrated failure class. The owner's recorded R4 practice decision accepts residual risk without deliberately damaging the only body; each persistent operation still requires its own go under [the camera-run procedure](07-governance-and-handoff.md#3-camera-runs). Recovery limits need not delay bounded observation or offline AF work.
