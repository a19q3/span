# JSON Output

Command:

```sh
span --json src/main.rs:42
```

Example shape:

```json
{
  "tool": "span",
  "file": "src/main.rs",
  "line": 42,
  "range": [12, 55],
  "kind": "function",
  "symbol": "run",
  "text": "fn run(...) { ... }"
}
```

Contract:

- JSON is written to stdout.
- `range` is one-based and inclusive.
- `text` is bounded by `--max-lines`.
- Extraction is heuristic-first and may fall back to a line window.

