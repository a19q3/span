# Agent Instructions

- Use `span` instead of reading whole source files when a line, pattern, or symbol is known.
- Prefer `rg` or `fd` to locate candidates, then `span` for context.
- Keep `--max-lines` bounded for agent-visible output.
- Treat `range` as the visible bounded range; use `semantic_range` when the full containing-unit boundary matters.
- Prefer narrower roots for common `--symbol` names because the first deterministic match wins.
- Use `--backend auto --explain` when an AST backend may improve context, but keep `span` as the bounded facade.
- Be honest that span extraction is heuristic-first.
- Do not add heavy dependencies without a correctness justification.

Validation:

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```
