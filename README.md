# span

`span` maps `FILE:LINE`, `--contains PATTERN`, or `--symbol NAME` to a bounded containing code block.

## Agent Value

Use `span` to avoid reading whole source files when the agent only needs the relevant function, Rust item, class, impl, Markdown fence, or fallback line window.

## Usage

```sh
span src/main.rs:42
span --contains "unwrap()" src/
span --symbol verify_proof_plan crates/
span --symbol FileInfo crates/
span --kind function --contains "panic!" src/
span --max-lines 40 --json src/main.rs:42
span --backend auto src/main.rs:42
span --backend ast-outline --symbol verify_proof_plan crates/
```

Backend options:

- `heuristic` (default): use `span`'s lightweight built-in extractor.
- `auto`: try `ast-outline`, then `ast-bro`, and fall back to the built-in extractor.
- `ast-outline`: delegate known symbol bodies to `ast-outline show FILE SYMBOL`.
- `ast-bro`: delegate known symbol bodies to `ast-bro show FILE SYMBOL`.

## JSON Output Contract

`span --json TARGET` writes a JSON span to stdout and exits non-zero when no span can be found.

Fields include:

- `tool`
- `backend`
- `file`
- `line`
- `range`
- `kind`
- `symbol`
- `text`

See [docs/json.md](docs/json.md).

## When To Use

- Compiler errors with a known file and line.
- Search results where the containing block matters.
- Symbol lookup where whole-file context would be wasteful.
- Agent workflows that need bounded code context.

## When Not To Use

- Full semantic name resolution.
- Call graphs or reference searches.
- Large codemods.
- Replacing `ast-outline`, `ast-bro`, `ast-grep`, or an LSP.

## Limitations

- v0.1 is heuristic-first with a safe line-window fallback.
- It is not a full parser.
- External AST backends are optional adapters, not required runtime dependencies.
- External backends are only used when `span` has a concrete symbol to delegate.
- `auto` silently falls back to the heuristic extractor when external backends are unavailable or unsuitable.
- Explicit external backends fail clearly when no concrete symbol can be inferred.
- Complex Rust macros, nested impls, and unusual formatting may produce approximate spans.
- Recursive `--contains` and `--symbol` searches are deterministic by path order.
- `--kind` filters candidates before returning a span.
