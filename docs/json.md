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
  "backend_reason": "heuristic backend selected explicitly",
  "fallback_used": false,
  "truncated": false,
  "file": "src/main.rs",
  "line": 42,
  "range": [12, 55],
  "semantic_range": [12, 55],
  "visible_range": [12, 55],
  "kind": "function",
  "symbol": "run",
  "text": "fn run(...) { ... }"
}
```

Contract:

- JSON is written to stdout.
- `backend` is `heuristic`, `ast-outline`, or `ast-bro`.
- `backend_reason` explains why that backend was selected.
- `fallback_used` is true when `auto` had to use a lower-priority backend or the heuristic fallback.
- `truncated` is true when `text` was capped by `--max-lines` or an external backend output cap.
- `range` is one-based, inclusive, and currently mirrors `visible_range` for backwards compatibility.
- `semantic_range` is the best-effort containing syntax unit before output capping.
- `visible_range` is the exact local range represented in `text` for the built-in heuristic backend.
- `text` is bounded by `--max-lines`.
- When heuristic output is truncated, the requested `line` is kept inside `visible_range`.
- Extraction is heuristic-first by default and may fall back to a line window.
- With an external backend, `semantic_range` and `visible_range` describe the local heuristic span used to identify the symbol, while `text` is the bounded delegated backend output.
- Recursive searches skip symlinked directories and stop at an internal depth guard.
- `--symbol` returns the first deterministic match by path and line order.
- External backends run with stdin closed, a timeout, capped stdout/stderr, and then `--max-lines` capping.

Backend doctor command:

```sh
span backend doctor --json
```

Example shape:

```json
{
  "tool": "span",
  "command": "backend doctor",
  "default_backend": "heuristic",
  "auto_order": ["ast-outline", "ast-bro", "heuristic"],
  "backends": [
    {
      "name": "ast-outline",
      "binary": "ast-outline",
      "available": true,
      "path": "/usr/local/bin/ast-outline",
      "help_ok": true,
      "help_status": "ok"
    }
  ]
}
```
