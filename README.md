# span

span -- extract syntax-bounded code context.

## Usage

```sh
span src/main.rs:42
span --contains "unwrap()" src/
span --symbol verify_proof_plan crates/
span --kind function --contains "panic!" src/
span --max-lines 40 --json src/main.rs:42
```

`span` maps a `FILE:LINE` target, first `--contains` match, or first `--symbol` match to a bounded containing code block using lightweight syntax heuristics, with a safe line-window fallback.
