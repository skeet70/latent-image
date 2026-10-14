"""Stock-boot/viewer entry with Rust execution and native provenance."""
from datetime import datetime, timezone
import json
from pathlib import Path
import sys
from uuid import uuid4

from emulator import boot as reference
from emulator.provenance import model_provenance
from emulator.provenance_active import active_device_provenance
from .provenance import provenance


class NativeBootSession(reference.BootSession):
    def run(self, options, *, _execute=True):
        prior_state = self.state
        try:
            return super().run(options, _execute=_execute)
        finally:
            if self.state is not prior_state and self.state is not None and "artifact_directory" in self.state:
                evidence = {**provenance(), "models": model_provenance(),
                            "active_devices": active_device_provenance(self.board),
                            "metrics": self.board._native.metrics()}
                self.state["execution_backend"] = dict(name="rust-unicorn",
                    provenance={key: value for key, value in evidence.items() if key != "metrics"},
                    metrics=evidence["metrics"])
                directory = Path(self.state["artifact_directory"])
                (directory / "native-backend.json").write_text(json.dumps(evidence, indent=2) + "\n")
                (directory / "state.json").write_text(json.dumps(self.state, indent=2) + "\n")


def main():
    # Use a separate evidence namespace unless the caller selects a fresh path.
    # No existing viewer is stopped/restarted, and no reference module is patched.
    if "--output" not in sys.argv and not any(arg.startswith("--output=") for arg in sys.argv[1:]):
        output = Path(reference.ROOT) / ".tools/emulator-runs" / (
            datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid4().hex[:8])
        sys.argv.extend(("--output", str(output)))
    return reference.main(session_factory=NativeBootSession)
