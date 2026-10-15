# Repository instructions

Read [GitHub Issues](https://github.com/skeet70/latent-image/issues), [current principles](docs/principles.md) and [specification index](spec/README.md) before extending this X-M1 autofocus research.

- Host work (source inspection, firmware analysis, emulation, synthetic tests, documentation) needs no approval. Anything on an emulated camera counts as host work.
- A camera operation runs only after the owner says go in chat. The owner runs the command; the model prepares it. Operations that change camera state need one independent review of new bytes before their first live run, and a go for a persistent change (firmware, configuration or calibration writes, End Setting) names the operation. The [camera-run procedure](spec/07-governance-and-handoff.md#3-camera-runs) is the whole process.
- Keep shell activation, imports, builds and tests free of camera I/O. Camera access lives only in the explicit live commands of `//transport:camera`.
- Use the checked-in `devenv` environment and Bazel build. Use Haskell for host tools and orchestration, and Haskell/Rust for models, simulation, fixtures, tests and native decision/execution code. Use C only where technically necessary at an engine or ABI boundary; existing C code does not justify new C. Preserve supported upstream programs in their original languages; add no project-authored Python. Retained Python emulator responsibilities are migration debt, not the default for new project logic.
- Never infer X-M1 addresses or behavior from sibling models. Label evidence and keep its scope; a passing scaffold does not accept a research gate or demonstrate recovery.
- Keep code, current rules, useful findings and open work easy to find. Use Git for superseded reports and history; do not add per-run narratives or hash inventories.
- Keep firmware, dumps, serials, raw camera logs and private artifacts under ignored `private-data/`. Synthetic fixtures need provenance.
- Approved agent models, as spawn selectors: `anthropic/claude-opus-5-5:high` or `openai-codex/gpt-6.1-sol:high` by default; `anthropic/claude-opus-5-5:high` or `openai-codex/gpt-daybreak-blue-latest:high` when a cyber classifier stops the assignment; `aironcore/qwen3.8-27b-nvfp4` for mechanically specified slices, reviewed by a stronger model; `xiaomi/mimo-v2.6-pro:high` and `deepseek/deepseek-flash:max` when the primary models rate-limit or are unavailable. No per-spawn approval needed. `omp models find <substring> --json` resolves a selector.

Update GitHub Issues when open work changes and the evidence ledger when a durable research claim changes.
