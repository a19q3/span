# span

span -- extract syntax-bounded code context.

## Usage

```sh
span src/main.rs:42
span --contains "unwrap()" src/
span --max-lines 40 --json src/main.rs:42
```

`span` maps a `FILE:LINE` target or first `--contains` match to a bounded containing code block using lightweight syntax heuristics, with a safe line-window fallback.
