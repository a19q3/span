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
- `truncated` is true when `text` was capped by `--max-lines`.
- `range` is one-based and inclusive.
- `text` is bounded by `--max-lines`.
- Extraction is heuristic-first by default and may fall back to a line window.
- With an external backend, `range` is the bounded local heuristic range used to identify the symbol, while `text` is the delegated backend output.

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
