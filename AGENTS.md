# Agent Instructions

- Use `span` instead of reading whole source files when a line, pattern, or symbol is known.
- Prefer `rg` or `fd` to locate candidates, then `span` for context.
- Keep `--max-lines` bounded for agent-visible output.
- Be honest that span extraction is heuristic-first.
- Do not add heavy dependencies without a correctness justification.

Validation:

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

