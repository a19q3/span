# JSON Output

Command:

```sh
span --json src/main.rs:42
```

Example shape:

```json
{
  "tool": "span",
  "backend": "heuristic",
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
- `backend` is `heuristic`, `ast-outline`, or `ast-bro`.
- `range` is one-based and inclusive.
- `text` is bounded by `--max-lines`.
- Extraction is heuristic-first by default and may fall back to a line window.
- With an external backend, `range` is the bounded local heuristic range used to identify the symbol, while `text` is the delegated backend output.
