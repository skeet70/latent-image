"""Interactive stock-boot checkpoints using Rust-owned Unicorn engines.

The shared warm workflow uses the Rust session directly.
New topology/prefix changes require cold boot,
and the checkpoint remains bound to these native engines in this process.
"""
from datetime import datetime, timezone
import json
from pathlib import Path
import sys
from uuid import uuid4

from tools import warm_boot as reference
from emulator.provenance import model_provenance
from .provenance import provenance


def main():
    supplied_output = next((arg.split("=", 1)[1] for arg in sys.argv[1:]
                            if arg.startswith("--output=")), None)
    if "--output" in sys.argv:
        supplied_output = sys.argv[sys.argv.index("--output") + 1]
    if supplied_output is None:
        output = reference.ROOT / ".tools/emulator-runs" / (
            datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-warm-" + uuid4().hex[:8])
        sys.argv.extend(("--output", str(output)))
    else:
        output = Path(supplied_output)
    preexisting_output = output.exists()
    evidence = {**provenance(), "models": model_provenance()}
    try:
        return reference.main()
    finally:
        if not preexisting_output and output.is_dir():
            path = output / "native-session.json"
            if not path.exists():
                path.write_text(json.dumps(evidence, indent=2) + "\n")


if __name__ == "__main__":
    main()
