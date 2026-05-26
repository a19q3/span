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
```

## JSON Output Contract

`span --json TARGET` writes a JSON span to stdout and exits non-zero when no span can be found.

Fields include:

- `tool`
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
- Cases where a real parser or LSP is required.

## Limitations

- v0.1 is heuristic-first with a safe line-window fallback.
- It is not a full parser.
- Complex Rust macros, nested impls, and unusual formatting may produce approximate spans.
- Recursive `--contains` and `--symbol` searches are deterministic by path order.
- `--kind` filters candidates before returning a span.
