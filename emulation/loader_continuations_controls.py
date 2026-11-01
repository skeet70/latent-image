#!/usr/bin/env python3
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import subprocess
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any, Callable, NewType

GuestAddress = NewType("GuestAddress", int)
EventId = NewType("EventId", int)
TcmRegisterWord = NewType("TcmRegisterWord", int)
Sha256 = NewType("Sha256", str)


class Verdict(Enum):
    ADMIT = 0
    REFUSE = 1


class Origin(Enum):
    ACTUAL = "actual-observation-mutation"
    SYNTHETIC = "serialized-synthetic-from-actual"


class CallKind(Enum):
    LOADER = "loader"
    IMAGE5_WAIT = "image5-wait"
    IMAGE5_ENTRY = "image5-entry"
    IMAGE5_SERVICE = "image5-service"
    IMAGE5_CONSTRUCTOR = "image5-constructor"
    PLAIN_PRODUCER = "plain-producer"
    PLAIN_SERVICE = "plain-service"
    PLAIN_HOOK = "plain-hook"
    DEFERRED = "deferred"
    TASK = "task"


class ResourceKey(Enum):
    LOADER_A = "loader-a"
    LOADER_B = "loader-b"
    IMAGE5 = "image5"
    IMAGE5_BSS = "image5-bss"
    PLAIN = "plain"
    PLAIN0 = "plain0"
    PLAIN2 = "plain2"
    PLAIN3 = "plain3"


class Access(Enum):
    CPU_FETCH = "cpu-fetch"
    CPU_READ = "cpu-read"
    CPU_WRITE = "cpu-write"
    HOST_READ = "host-read"
    HOST_WRITE = "host-write"
    MODEL_BUS_READ = "model-bus-read"
    MODEL_BUS_WRITE = "model-bus-write"


class ClockMode(Enum):
    BEGIN = "begin"
    AFTER_END = "after-end"
    REVERSE = "reverse"
    RESUMED = "resumed"
    END = "end"


class ExtentId(Enum):
    ZERO = 0
    TWO = 2
    THREE = 3


@dataclass(frozen=True)
class CodeImage:
    start: GuestAddress
    path: Path


CODE_IMAGES = (
    CodeImage(GuestAddress(0x61C400), Path("lz/id4-0061c400.bin")),
    CodeImage(GuestAddress(0xBF1A00), Path("lz/id5-00bf1a00.bin")),
)


def number(value: Any) -> int:
    return int(value, 16) if isinstance(value, str) else int(value)


def encoded(template: Any, value: int) -> Any:
    return f"0x{value:x}" if isinstance(template, str) else value


def emit(path: Path, value: Any) -> Sha256:
    data = (json.dumps(value, indent=2) + "\n").encode()
    with path.open("xb") as handle:
        handle.write(data)
    return Sha256(hashlib.sha256(data).hexdigest())


@dataclass
class Fixture:
    root: Path
    receipt: dict[str, Any]
    trace: list[dict[str, Any]]
    watches: list[dict[str, Any]]
    analysis: Path

    @classmethod
    def load(cls, root: Path, analysis: Path) -> Fixture:
        receipt = json.loads((root / "continuations.json").read_bytes())
        trace = json.loads((root / receipt["trace"]["path"]).read_bytes())
        watches = [json.loads((root / epoch["file"]["path"]).read_bytes()) for epoch in receipt["epochs"]]
        return cls(root, receipt, trace, watches, analysis)

    def clone(self) -> Fixture:
        return Fixture(self.root, copy.deepcopy(self.receipt), copy.deepcopy(self.trace), copy.deepcopy(self.watches), self.analysis)

    def event(self, index: EventId) -> dict[str, Any]:
        return self.receipt["events"][index]

    def first(self, pc: GuestAddress) -> EventId:
        return EventId(next(index for index, event in enumerate(self.receipt["events"]) if number(event["pc"]) == pc))

    def calls(self, kind: CallKind) -> list[int]:
        return [index for index, call in enumerate(self.receipt["calls"]) if CallKind(call["kind"]) is kind]

    def sync(self, index: EventId) -> None:
        event = self.event(index)
        registers = event["registers"]
        raw = self.trace[index]
        raw.update(address=f"0x{number(event['pc']):x}", core=event["core"], instructions=event["instructions"], cpsr=registers["cpsr"], sp=f"0x{number(registers['sp']):x}", lr=f"0x{number(registers['lr']):x}")
        raw["registers"] = {f"r{register}": f"0x{value:x}" for register, value in enumerate(registers["r"])}

    def code(self, index: EventId, pc: GuestAddress) -> None:
        event = self.event(index)
        physical = pc & ~0x40000000
        for image in CODE_IMAGES:
            binding = next((source for source in self.receipt["identity"]["sources"] if number(source["start"]) == image.start and image.start <= physical < image.start + number(source["bytes"])), None)
            if binding is None:
                continue
            with (self.analysis / image.path).open("rb") as handle:
                handle.seek(physical - image.start)
                data = handle.read(4)
            if len(data) != 4:
                raise ValueError("source word is outside its bound image")
            event["pc"] = encoded(event["pc"], pc)
            event["source"] = binding["kind"]
            mapping = event["code"]["mapping"]
            mapping["guest"] = encoded(mapping["guest"], pc)
            mapping["physical"] = encoded(mapping["physical"], physical)
            event["code"]["values"] = [int.from_bytes(data, "little")]
            self.sync(index)
            return
        raise ValueError("control source address is outside image4/image5")

    def remove_events(self, removed: set[EventId]) -> None:
        events = self.receipt["events"]
        remap = {old: new for new, old in enumerate(index for index in range(len(events)) if index not in removed)}
        for call in self.receipt["calls"]:
            for field in ("before", "entry", "returned"):
                if call[field] is not None:
                    call[field] = remap[call[field]]
        for sample in self.receipt["snapshots"]:
            sample["event"] = remap[sample["event"]]
        self.receipt["events"] = [event for index, event in enumerate(events) if index not in removed]
        self.trace = [hit for index, hit in enumerate(self.trace) if index not in removed]
        for index, event in enumerate(self.receipt["events"]):
            event["id"] = index

    def write(self, output: Path) -> None:
        output.mkdir()
        excluded = {"continuations.json", "verification.json", "observer-error.json", self.receipt["trace"]["path"]}
        excluded.update(epoch["file"]["path"] for epoch in self.receipt["epochs"])
        for source in self.root.iterdir():
            if source.is_file() and source.name not in excluded:
                (output / source.name).hardlink_to(source)
        trace_name = "control-trace.json"
        self.receipt["trace"] = {"path": trace_name, "sha256": emit(output / trace_name, self.trace)}
        for index, (epoch, watch) in enumerate(zip(self.receipt["epochs"], self.watches, strict=True)):
            name = f"control-watch-{index}.json"
            epoch["file"] = {"path": name, "sha256": emit(output / name, watch)}
            epoch["record_count"] = len(watch["records"])
        emit(output / "continuations.json", self.receipt)


def omit_call(fixture: Fixture, kind: CallKind) -> None:
    del fixture.receipt["calls"][fixture.calls(kind)[0]]


def deferred(fixture: Fixture) -> None:
    hooks = fixture.calls(CallKind.PLAIN_HOOK)
    extent3 = [index for index in hooks if ExtentId(fixture.event(fixture.receipt["calls"][index]["before"])["registers"]["r"][7]) is ExtentId.THREE]
    selected = extent3[1]
    call = fixture.receipt["calls"][selected]
    call["kind"] = CallKind.DEFERRED.value
    call["continuation"] = encoded(call["continuation"], 0x73BEB0)
    fixture.code(call["before"], GuestAddress(0x73BEAC))
    for index in (call["entry"], call["returned"]):
        event = fixture.event(index)
        event["registers"]["lr"] = encoded(event["registers"]["lr"], 0x73BEB0)
        fixture.sync(index)
    fixture.code(call["returned"], GuestAddress(0x73BEB0))


def missing_deferred(fixture: Fixture) -> None:
    deferred(fixture)
    omit_call(fixture, CallKind.DEFERRED)


def phases_out_of_order(fixture: Fixture) -> None:
    selected = []
    for index, call in enumerate(fixture.receipt["calls"]):
        if CallKind(call["kind"]) is CallKind.LOADER and call["before"] is not None and number(fixture.event(call["before"])["pc"]) in (0x40A34, 0x40A3C):
            selected.append(index)
    if len(selected) != 2:
        raise ValueError("phase1/3 source calls absent")
    selected.sort(key=lambda index: fixture.receipt["calls"][index]["before"])
    for index, pc, phase in [(selected[0], 0x40A3C, 3), (selected[1], 0x40A34, 1)]:
        call = fixture.receipt["calls"][index]
        call["continuation"] = encoded(call["continuation"], pc + 4)
        for event_id in (call["before"], call["entry"]):
            fixture.event(event_id)["registers"]["r"][0] = phase
            fixture.sync(event_id)
        entered = fixture.event(call["entry"])
        entered["registers"]["lr"] = encoded(entered["registers"]["lr"], pc + 4)
        fixture.sync(call["entry"])
        fixture.code(call["before"], GuestAddress(pc))
        fixture.code(call["returned"], GuestAddress(pc + 4))
    calls = fixture.receipt["calls"]
    calls[selected[0]], calls[selected[1]] = calls[selected[1]], calls[selected[0]]


def constructors_out_of_order(fixture: Fixture) -> None:
    selected = fixture.calls(CallKind.IMAGE5_CONSTRUCTOR)[:2]
    if len(selected) != 2:
        raise ValueError("two constructor calls absent")
    targets = [number(fixture.receipt["calls"][index]["target"]) for index in selected]
    for index, target in zip(selected, reversed(targets), strict=True):
        call = fixture.receipt["calls"][index]
        call["target"] = encoded(call["target"], target)
        for event_id in (call["before"], call["entry"]):
            fixture.event(event_id)["registers"]["r"][3] = target
            fixture.sync(event_id)
        fixture.code(call["entry"], GuestAddress(target))
    calls = fixture.receipt["calls"]
    calls[selected[0]], calls[selected[1]] = calls[selected[1]], calls[selected[0]]


def omit_projection(fixture: Fixture, resource: ResourceKey, pc: GuestAddress | None = None) -> None:
    index = next(index for index, sample in enumerate(fixture.receipt["snapshots"]) if ResourceKey(sample["resource"]) is resource and (pc is None or number(fixture.event(sample["event"])["pc"]) == pc))
    del fixture.receipt["snapshots"][index]


def creation(fixture: Fixture) -> dict[str, Any]:
    event = fixture.event(fixture.first(GuestAddress(0x43758)))
    return next(view for view in event["state"] if number(view["mapping"]["guest"]) == event["registers"]["r"][2] and len(view["values"]) == 3)


def wrong_creation_argument(fixture: Fixture) -> None:
    creation(fixture)["values"][1] ^= 1


def wrong_creation_target(fixture: Fixture) -> None:
    creation(fixture)["values"][2] += 4


def wrong_cleanup(fixture: Fixture) -> None:
    index = fixture.first(GuestAddress(0x43758))
    registers = fixture.event(index)["registers"]
    registers["lr"] = encoded(registers["lr"], number(registers["lr"]) + 4)
    fixture.sync(index)


@dataclass(frozen=True)
class CpuRows:
    epoch: int
    indices: list[int]


def safe_cpu_rows(fixture: Fixture) -> CpuRows:
    for epoch, watch in enumerate(fixture.watches):
        rows = [index for index, row in enumerate(watch["records"]) if Access(row["source"]) is Access.CPU_WRITE and not any(low <= number(row["physical"]) < high for low, high in [(0x409AC, 0x40B00), (0xD09AC, 0xD0B00), (0x63450, 0x63460), (0xF3410, 0xF3420)])]
        if len(rows) >= 2:
            return CpuRows(epoch, rows)
    raise ValueError("two non-carrier CPU writes absent")


def clock(fixture: Fixture, mode: ClockMode) -> None:
    selected = safe_cpu_rows(fixture)
    epoch = fixture.receipt["epochs"][selected.epoch]
    indices = selected.indices
    watch = fixture.watches[selected.epoch]
    rows = watch["records"]
    if mode is ClockMode.REVERSE:
        first = indices[0]
        second = next(index for index in indices[1:] if rows[index]["instructions"] != rows[first]["instructions"])
        rows[first]["instructions"], rows[second]["instructions"] = rows[second]["instructions"], rows[first]["instructions"]
        return
    index = indices[-1] if mode is ClockMode.END else indices[0]
    row = rows[index]
    row["instructions"] = {ClockMode.BEGIN: epoch["start"], ClockMode.AFTER_END: epoch["end"] + 1, ClockMode.RESUMED: epoch["start"] + 1, ClockMode.END: epoch["end"]}[mode]
    if mode in (ClockMode.RESUMED, ClockMode.END):
        del rows[index]
        rows.insert(0 if mode is ClockMode.RESUMED else len(rows), row)


def phase_call(fixture: Fixture, site: GuestAddress) -> dict[str, Any]:
    return next(call for call in fixture.receipt["calls"] if CallKind(call["kind"]) is CallKind.LOADER and call["before"] is not None and number(fixture.event(call["before"])["pc"]) == site)


def wrong_phase_argument(fixture: Fixture) -> None:
    call = phase_call(fixture, GuestAddress(0x40A34))
    for index in (call["before"], call["entry"]):
        fixture.event(index)["registers"]["r"][0] = 4
        fixture.sync(index)


def wrong_dispatch_target(fixture: Fixture) -> None:
    call = phase_call(fixture, GuestAddress(0x40A34))
    target = GuestAddress(0x69CDFC)
    call["target"] = encoded(call["target"], target)
    for index in (call["before"], call["entry"], call["returned"]):
        fixture.event(index)["registers"]["r"][4] = target
        fixture.sync(index)
    fixture.code(call["entry"], target)


def wrong_returned_code(fixture: Fixture) -> None:
    call = phase_call(fixture, GuestAddress(0x40A68))
    publisher = next(other for other in fixture.receipt["calls"] if other["before"] == call["returned"])
    ready = next(other for other in fixture.receipt["calls"] if other["before"] == publisher["returned"])
    for index in (call["returned"], publisher["entry"], publisher["returned"], ready["entry"]):
        fixture.event(index)["registers"]["r"][0] = 0x69CC70
        fixture.sync(index)


def wrong_saved_pc(fixture: Fixture) -> None:
    event = fixture.event(fixture.first(GuestAddress(0x436E8)))
    view = next(view for view in event["state"] if number(view["mapping"]["guest"]) == event["registers"]["r"][1] + 24 and len(view["values"]) == 2)
    view["values"][1] += 4


def out_of_range_write(fixture: Fixture) -> None:
    selected = safe_cpu_rows(fixture)
    epoch = fixture.receipt["epochs"][selected.epoch]
    rows = fixture.watches[selected.epoch]["records"]
    row = copy.deepcopy(rows[selected.indices[-1]])
    row["address"] = encoded(row["address"], 0x20000000)
    row["physical"] = encoded(row["physical"], 0x20000000)
    row["instructions"] = epoch["end"]
    rows.append(row)


def tcm(fixture: Fixture, bank: TcmRegisterWord) -> None:
    fixture.receipt["events"][0]["code"]["mapping"]["tcm"][0][0] = bank


@dataclass(frozen=True)
class Case:
    name: str
    verdict: Verdict
    mutate: Callable[[Fixture], None]
    origin: Origin = Origin.ACTUAL


def move_bss_projection(fixture: Fixture) -> None:
    sample = next(sample for sample in fixture.receipt["snapshots"] if ResourceKey(sample["resource"]) is ResourceKey.IMAGE5_BSS and number(fixture.event(sample["event"])["pc"]) == 0xD985A4)
    sample["event"] = fixture.first(GuestAddress(0x6A9840))


def cases(baseline: Fixture) -> list[Case]:
    result = [
        Case("call-record-order-is-not-execution-order", Verdict.ADMIT, lambda fixture: fixture.receipt["calls"].reverse()),
        Case("wrong-phase-argument", Verdict.REFUSE, wrong_phase_argument),
        Case("wrong-dispatch-target", Verdict.REFUSE, wrong_dispatch_target, Origin.SYNTHETIC),
        Case("wrong-returned-code", Verdict.REFUSE, wrong_returned_code, Origin.SYNTHETIC),
        Case("wrong-saved-pc", Verdict.REFUSE, wrong_saved_pc),
        Case("out-of-range-write", Verdict.REFUSE, out_of_range_write, Origin.SYNTHETIC),
        Case("missing-task-call", Verdict.REFUSE, lambda fixture: omit_call(fixture, CallKind.TASK)),
        Case("missing-plain-hook-call", Verdict.REFUSE, lambda fixture: omit_call(fixture, CallKind.PLAIN_HOOK)),
        Case("synthetic-deferred-call", Verdict.ADMIT, deferred, Origin.SYNTHETIC),
        Case("missing-deferred-call", Verdict.REFUSE, missing_deferred, Origin.SYNTHETIC),
        Case("phase-raw-order", Verdict.REFUSE, phases_out_of_order, Origin.SYNTHETIC),
        Case("constructor-raw-order", Verdict.REFUSE, constructors_out_of_order, Origin.SYNTHETIC),
        Case("missing-loader-return-bss", Verdict.REFUSE, lambda fixture: omit_projection(fixture, ResourceKey.IMAGE5_BSS, GuestAddress(0x6A9840))),
        Case("missing-first-image5-image", Verdict.REFUSE, lambda fixture: omit_projection(fixture, ResourceKey.IMAGE5, GuestAddress(0xD985A4))),
        Case("missing-first-image5-bss", Verdict.REFUSE, lambda fixture: omit_projection(fixture, ResourceKey.IMAGE5_BSS, GuestAddress(0xD985A4))),
        Case("missing-first-image5-plain", Verdict.REFUSE, lambda fixture: omit_projection(fixture, ResourceKey.PLAIN, GuestAddress(0xD985A4))),
        Case("projection-attached-to-wrong-boundary", Verdict.REFUSE, move_bss_projection),
        Case("missing-trampoline-transfer", Verdict.REFUSE, lambda fixture: fixture.remove_events({fixture.first(GuestAddress(0x43758))})),
        Case("wrong-trampoline-argument", Verdict.REFUSE, wrong_creation_argument),
        Case("wrong-trampoline-target", Verdict.REFUSE, wrong_creation_target),
        Case("wrong-trampoline-cleanup", Verdict.REFUSE, wrong_cleanup),
        Case("cpu-clock-at-begin", Verdict.REFUSE, lambda fixture: clock(fixture, ClockMode.BEGIN)),
        Case("cpu-clock-after-end", Verdict.REFUSE, lambda fixture: clock(fixture, ClockMode.AFTER_END)),
        Case("cpu-clock-reversal", Verdict.REFUSE, lambda fixture: clock(fixture, ClockMode.REVERSE)),
        Case("cpu-clock-at-end", Verdict.ADMIT, lambda fixture: clock(fixture, ClockMode.END), Origin.SYNTHETIC),
        Case("cpu-clock-resumed-begin-plus-one", Verdict.ADMIT, lambda fixture: clock(fixture, ClockMode.RESUMED), Origin.SYNTHETIC),
        Case("misaligned-tcm-window", Verdict.REFUSE, lambda fixture: tcm(fixture, TcmRegisterWord(0x00201011))),
        Case("tcm-window-past-address-space", Verdict.REFUSE, lambda fixture: tcm(fixture, TcmRegisterWord(0xFFFFF039))),
        Case("tcm-window-exact-address-space-end", Verdict.ADMIT, lambda fixture: tcm(fixture, TcmRegisterWord(0xFF800039)), Origin.SYNTHETIC),
    ]
    for extent in (ResourceKey.PLAIN0, ResourceKey.PLAIN2, ResourceKey.PLAIN3):
        result.append(Case(f"missing-extent-{extent.value}", Verdict.REFUSE, lambda fixture, extent=extent: omit_projection(fixture, extent)))
    for pc in (GuestAddress(0x6A8C04), GuestAddress(0x6A9734)):
        if any(number(event["pc"]) == pc for event in baseline.receipt["events"]):
            result.append(Case(f"missing-observed-bss-{pc:x}", Verdict.REFUSE, lambda fixture, pc=pc: omit_projection(fixture, ResourceKey.IMAGE5_BSS, pc)))
    return result


def invoke(observer: Path, analysis: Path, fixture: Path, output: Path) -> int:
    result = subprocess.run([str(observer), "--verify", str(fixture), "--analysis", str(analysis)], capture_output=True, check=False)
    with output.with_suffix(".stdout").open("xb") as handle:
        handle.write(result.stdout)
    with output.with_suffix(".stderr").open("xb") as handle:
        handle.write(result.stderr)
    return result.returncode


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--observer", type=Path, required=True)
    parser.add_argument("--analysis", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir()
    baseline = invoke(args.observer.resolve(), args.analysis.resolve(), args.baseline.resolve(), args.output / "baseline")
    if baseline != 0:
        emit(args.output / "controls.json", {"schema": 1, "baseline_exit": baseline, "cases": [], "passed": False})
        return 1
    original = Fixture.load(args.baseline.resolve(), args.analysis.resolve())
    outcomes = []
    for case in cases(original):
        fixture = original.clone()
        case.mutate(fixture)
        directory = args.output / case.name
        fixture.write(directory)
        actual = invoke(args.observer.resolve(), args.analysis.resolve(), directory.resolve(), args.output / case.name)
        outcomes.append({"name": case.name, "scope": case.origin.value, "expected_exit": case.verdict.value, "actual_exit": actual, "passed": actual == case.verdict.value})
        emit(args.output / f"{case.name}.result.json", outcomes[-1])
    manifest = {"schema": 1, "baseline_exit": baseline, "observer_sha256": hashlib.sha256(args.observer.read_bytes()).hexdigest(), "controls_source_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "baseline_receipt_sha256": hashlib.sha256((args.baseline / "continuations.json").read_bytes()).hexdigest(), "cases": outcomes, "passed": all(outcome["passed"] for outcome in outcomes)}
    emit(args.output / "controls.json", manifest)
    return 0 if manifest["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
