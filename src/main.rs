use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, Read};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_MAX_LINES: usize = 80;
const CONTEXT_RADIUS: usize = 20;
const MAX_SEARCH_DEPTH: usize = 64;
const EXTERNAL_BACKEND_TIMEOUT: Duration = Duration::from_secs(5);
const EXTERNAL_STDOUT_LIMIT: usize = 1024 * 1024;
const EXTERNAL_STDERR_LIMIT: usize = 64 * 1024;

#[derive(Debug)]
struct Args {
    target: Target,
    max_lines: usize,
    kind: Option<String>,
    backend: Backend,
    explain: bool,
    json: bool,
}

#[derive(Debug)]
enum Cli {
    Span(Args),
    BackendDoctor { json: bool },
}

#[derive(Debug)]
enum Target {
    Position(String),
    Contains { pattern: String, root: PathBuf },
    Symbol { symbol: String, root: PathBuf },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Backend {
    Heuristic,
    Auto,
    AstOutline,
    AstBro,
}

#[derive(Clone, Debug)]
struct Span {
    start: usize,
    end: usize,
    kind: &'static str,
    symbol: String,
}

#[derive(Debug)]
struct SpanView {
    semantic: Span,
    visible: Span,
    truncated: bool,
}

#[derive(Debug)]
struct ExternalSpan {
    backend: &'static str,
    text: String,
    truncated: bool,
}

#[derive(Debug)]
struct BackendSelection {
    backend: &'static str,
    reason: String,
    fallback_used: bool,
    external: Option<ExternalSpan>,
    truncated: bool,
}

#[derive(Debug)]
struct BackendProbe {
    name: &'static str,
    binary: &'static str,
    available: bool,
    path: Option<String>,
    help_ok: bool,
    help_status: String,
}

#[derive(Debug)]
struct LimitedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn main() {
    let cli = match parse_cli(env::args().skip(1).collect()) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("span: {message}");
            print_usage_stderr();
            process::exit(2);
        }
    };

    match run_cli(&cli) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("span: {error}");
            process::exit(1);
        }
    }
}

fn parse_cli(input: Vec<String>) -> Result<Cli, String> {
    if input.first().map(String::as_str) == Some("backend") {
        parse_backend_command(input.into_iter().skip(1))
    } else {
        parse_args(input).map(Cli::Span)
    }
}

fn parse_backend_command<I>(input: I) -> Result<Cli, String>
where
    I: IntoIterator<Item = String>,
{
    let mut iter = input.into_iter();
    let Some(command) = iter.next() else {
        return Err("backend requires a subcommand".to_string());
    };

    if command != "doctor" {
        return Err(format!("unknown backend subcommand: {command}"));
    }

    let mut json = false;
    for arg in iter {
        match arg.as_str() {
            "--json" => json = true,
            "-h" | "--help" => {
                println!("usage: span backend doctor [--json]");
                process::exit(0);
            }
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }

    Ok(Cli::BackendDoctor { json })
}

fn parse_args<I>(input: I) -> Result<Args, String>
where
    I: IntoIterator<Item = String>,
{
    let mut max_lines = DEFAULT_MAX_LINES;
    let mut kind = None;
    let mut backend = Backend::Heuristic;
    let mut explain = false;
    let mut json = false;
    let mut contains = None;
    let mut symbol = None;
    let mut position = None;
    let mut root = None;
    let mut iter = input.into_iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--max-lines" => {
                max_lines = iter
                    .next()
                    .ok_or_else(|| "--max-lines requires a value".to_string())?
                    .parse()
                    .map_err(|_| "--max-lines requires a positive integer".to_string())?;
                if max_lines == 0 {
                    return Err("--max-lines requires a positive integer".to_string());
                }
            }
            "--kind" => {
                kind = Some(
                    iter.next()
                        .ok_or_else(|| "--kind requires a value".to_string())?,
                );
            }
            "--backend" => {
                backend = parse_backend(
                    &iter
                        .next()
                        .ok_or_else(|| "--backend requires a value".to_string())?,
                )?;
            }
            "--contains" => {
                contains = Some(
                    iter.next()
                        .ok_or_else(|| "--contains requires a pattern".to_string())?,
                );
            }
            "--symbol" => {
                symbol = Some(
                    iter.next()
                        .ok_or_else(|| "--symbol requires a name".to_string())?,
                );
            }
            "--json" => json = true,
            "--explain" => explain = true,
            "-h" | "--help" => {
                print_usage_stdout();
                process::exit(0);
            }
            _ if (contains.is_some() || symbol.is_some()) && root.is_none() => {
                root = Some(PathBuf::from(arg));
            }
            _ if contains.is_none() && symbol.is_none() && position.is_none() => {
                position = Some(arg);
            }
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }

    if contains.is_some() && symbol.is_some() {
        return Err("--contains and --symbol cannot be used together".to_string());
    }

    let target = if let Some(pattern) = contains {
        Target::Contains {
            pattern,
            root: root.unwrap_or_else(|| PathBuf::from(".")),
        }
    } else if let Some(symbol) = symbol {
        Target::Symbol {
            symbol,
            root: root.unwrap_or_else(|| PathBuf::from(".")),
        }
    } else {
        Target::Position(position.ok_or_else(|| "missing FILE:LINE target".to_string())?)
    };

    Ok(Args {
        target,
        max_lines,
        kind,
        backend,
        explain,
        json,
    })
}

fn print_usage_stdout() {
    println!(
        "usage: span [--max-lines N] [--kind KIND] [--backend NAME] [--explain] [--json] FILE:LINE"
    );
    println!(
        "       span [--max-lines N] [--kind KIND] [--backend NAME] [--explain] [--json] --contains PATTERN [PATH]"
    );
    println!(
        "       span [--max-lines N] [--kind KIND] [--backend NAME] [--explain] [--json] --symbol NAME [PATH]"
    );
    println!("       span backend doctor [--json]");
    println!("       backend: heuristic | auto | ast-outline | ast-bro");
}

fn print_usage_stderr() {
    eprintln!(
        "usage: span [--max-lines N] [--kind KIND] [--backend NAME] [--explain] [--json] FILE:LINE"
    );
    eprintln!(
        "       span [--max-lines N] [--kind KIND] [--backend NAME] [--explain] [--json] --contains PATTERN [PATH]"
    );
    eprintln!(
        "       span [--max-lines N] [--kind KIND] [--backend NAME] [--explain] [--json] --symbol NAME [PATH]"
    );
    eprintln!("       span backend doctor [--json]");
}

fn parse_backend(value: &str) -> Result<Backend, String> {
    match value {
        "heuristic" => Ok(Backend::Heuristic),
        "auto" => Ok(Backend::Auto),
        "ast-outline" => Ok(Backend::AstOutline),
        "ast-bro" => Ok(Backend::AstBro),
        _ => Err(format!(
            "unknown backend {value}; expected heuristic, auto, ast-outline, or ast-bro"
        )),
    }
}

fn print_backend_doctor(json: bool) {
    let probes = backend_probes();
    if json {
        let backends = probes
            .iter()
            .map(|probe| {
                format!(
                    "{{\"name\":\"{}\",\"binary\":\"{}\",\"available\":{},\"path\":{},\"help_ok\":{},\"help_status\":\"{}\"}}",
                    probe.name,
                    probe.binary,
                    probe.available,
                    json_optional_string(probe.path.as_deref()),
                    probe.help_ok,
                    json_escape(&probe.help_status)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"tool\":\"span\",\"command\":\"backend doctor\",\"default_backend\":\"heuristic\",\"auto_order\":[\"ast-outline\",\"ast-bro\",\"heuristic\"],\"backends\":[{backends}]}}"
        );
        return;
    }

    println!("default backend: heuristic");
    println!("auto order: ast-outline -> ast-bro -> heuristic");
    for probe in probes {
        println!(
            "{}: {}",
            probe.name,
            if probe.available { "found" } else { "missing" }
        );
        println!("  path: {}", probe.path.as_deref().unwrap_or("<not found>"));
        println!("  help: {}", probe.help_status);
    }
}

fn backend_probes() -> Vec<BackendProbe> {
    [Backend::AstOutline, Backend::AstBro]
        .into_iter()
        .filter_map(external_backend_command)
        .map(|(name, binary)| probe_backend(name, binary))
        .collect()
}

fn probe_backend(name: &'static str, binary: &'static str) -> BackendProbe {
    let path = find_executable_in_path(binary);
    let Some(path) = path else {
        return BackendProbe {
            name,
            binary,
            available: false,
            path: None,
            help_ok: false,
            help_status: "not-run".to_string(),
        };
    };

    let (help_ok, help_status) = match Command::new(binary).arg("--help").output() {
        Ok(output) if output.status.success() => (true, "ok".to_string()),
        Ok(output) => (false, format!("exit-{}", output.status.code().unwrap_or(1))),
        Err(error) => (false, format!("start-error: {error}")),
    };

    BackendProbe {
        name,
        binary,
        available: true,
        path: Some(path.display().to_string()),
        help_ok,
        help_status,
    }
}

fn find_executable_in_path(binary: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH").unwrap_or_default();
    for dir in env::split_paths(&path) {
        let candidate = dir.join(binary);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
        && fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn run_cli(cli: &Cli) -> Result<(), String> {
    match cli {
        Cli::Span(args) => run(args),
        Cli::BackendDoctor { json } => {
            print_backend_doctor(*json);
            Ok(())
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let candidates = match &args.target {
        Target::Position(target) => {
            let (file, line) = parse_target(target)?;
            vec![(file.to_string(), line)]
        }
        Target::Contains { pattern, root } => find_contains(root, pattern)?,
        Target::Symbol { symbol, root } => find_symbol(root, symbol)?,
    };

    let mut kind_mismatches = Vec::new();
    for (file, line) in candidates {
        let text = fs::read_to_string(&file).map_err(|error| format!("{file}: {error}"))?;
        let lines: Vec<&str> = text.lines().collect();

        if line == 0 || line > lines.len() {
            return Err(format!("{file}: line {line} is outside 1..{}", lines.len()));
        }

        let span = find_span(&file, &lines, line);
        if let Some(kind) = &args.kind {
            if span.kind != kind {
                kind_mismatches.push(span.kind);
                continue;
            }
        }
        let view = cap_span(span, line, args.max_lines, lines.len());
        let selection = select_backend(
            args.backend,
            &file,
            &view.semantic,
            args.max_lines,
            view.truncated,
        )?;

        if args.explain {
            eprintln!("backend: {}", selection.backend);
            eprintln!("backend reason: {}", selection.reason);
            eprintln!(
                "fallback used: {}",
                if selection.fallback_used { "yes" } else { "no" }
            );
            eprintln!(
                "truncated: {}",
                if selection.truncated { "yes" } else { "no" }
            );
        }

        if args.json {
            print_json(&file, line, &lines, &view, &selection);
        } else if let Some(external) = &selection.external {
            print_external_human(&file, &view, external);
        } else {
            print_human(&file, line, &lines, &view);
        }

        return Ok(());
    }

    if let Some(kind) = &args.kind {
        if !kind_mismatches.is_empty() {
            return Err(format!("no matched span had expected kind {kind}"));
        }
    }

    Err("no matching span found".to_string())
}

fn parse_target(target: &str) -> Result<(&str, usize), String> {
    let (file, line) = target
        .rsplit_once(':')
        .ok_or_else(|| "target must be FILE:LINE".to_string())?;
    let line = line
        .parse()
        .map_err(|_| "line must be a positive integer".to_string())?;
    Ok((file, line))
}

fn find_contains(root: &Path, pattern: &str) -> Result<Vec<(String, usize)>, String> {
    let mut matches = Vec::new();
    let mut visited = HashSet::new();
    collect_matches(root, pattern, &mut matches, &mut visited, 0)?;
    matches.sort();
    if matches.is_empty() {
        Err(format!("pattern not found: {pattern}"))
    } else {
        Ok(matches)
    }
}

fn find_symbol(root: &Path, symbol: &str) -> Result<Vec<(String, usize)>, String> {
    let mut matches = Vec::new();
    let mut visited = HashSet::new();
    collect_symbol_matches(root, symbol, &mut matches, &mut visited, 0)?;
    matches.sort();
    if matches.is_empty() {
        Err(format!("symbol not found: {symbol}"))
    } else {
        Ok(matches)
    }
}

fn collect_symbol_matches(
    path: &Path,
    symbol: &str,
    matches: &mut Vec<(String, usize)>,
    visited: &mut HashSet<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_SEARCH_DEPTH {
        return Ok(());
    }

    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    if metadata.is_dir() {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !visited.insert(canonical) {
            return Ok(());
        }

        for entry in sorted_entries(path)? {
            if is_skipped_entry(&entry) {
                continue;
            }
            collect_symbol_matches(&entry, symbol, matches, visited, depth + 1)?;
        }
        return Ok(());
    }

    if !metadata.is_file() {
        return Ok(());
    }

    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };

    for (index, line) in text.lines().enumerate() {
        if looks_like_symbol(line) && symbol_name(line) == symbol {
            matches.push((path.to_string_lossy().to_string(), index + 1));
        }
    }

    Ok(())
}

fn collect_matches(
    path: &Path,
    pattern: &str,
    matches: &mut Vec<(String, usize)>,
    visited: &mut HashSet<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_SEARCH_DEPTH {
        return Ok(());
    }

    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }

    if metadata.is_dir() {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !visited.insert(canonical) {
            return Ok(());
        }

        for entry in sorted_entries(path)? {
            if is_skipped_entry(&entry) {
                continue;
            }
            collect_matches(&entry, pattern, matches, visited, depth + 1)?;
        }
        return Ok(());
    }

    if !metadata.is_file() {
        return Ok(());
    }

    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };

    for (index, line) in text.lines().enumerate() {
        if line.contains(pattern) {
            matches.push((path.to_string_lossy().to_string(), index + 1));
        }
    }

    Ok(())
}

fn sorted_entries(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut entries = fs::read_dir(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    Ok(entries)
}

fn is_skipped_entry(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, ".git" | "target" | "node_modules"))
}

fn find_span(file: &str, lines: &[&str], line: usize) -> Span {
    if Path::new(file)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        if let Some(span) = markdown_fence_span(lines, line) {
            return span;
        }
    }

    if let Some(span) = metadata_forward_span(lines, line) {
        return span;
    }

    if let Some(span) = syntactic_start_span(lines, line) {
        return span;
    }

    let start = line.saturating_sub(CONTEXT_RADIUS).max(1);
    let end = (line + CONTEXT_RADIUS).min(lines.len());
    Span {
        start,
        end,
        kind: "context",
        symbol: "<line-window>".to_string(),
    }
}

fn select_backend(
    backend: Backend,
    file: &str,
    span: &Span,
    max_lines: usize,
    local_truncated: bool,
) -> Result<BackendSelection, String> {
    if backend == Backend::Heuristic {
        return Ok(BackendSelection {
            backend: "heuristic",
            reason: "heuristic backend selected explicitly".to_string(),
            fallback_used: false,
            external: None,
            truncated: local_truncated,
        });
    }

    if !is_external_symbol(&span.symbol) {
        return match backend {
            Backend::Auto | Backend::Heuristic => Ok(BackendSelection {
                backend: "heuristic",
                reason: format!(
                    "auto fell back to heuristic because no concrete symbol was inferred ({})",
                    span.symbol
                ),
                fallback_used: true,
                external: None,
                truncated: local_truncated,
            }),
            Backend::AstOutline | Backend::AstBro => Err(format!(
                "backend {} requires a concrete symbol; got {}",
                backend_name(backend),
                span.symbol
            )),
        };
    }

    match backend {
        Backend::Heuristic => unreachable!("heuristic handled above"),
        Backend::Auto => {
            for (index, backend) in [Backend::AstOutline, Backend::AstBro]
                .into_iter()
                .enumerate()
            {
                if let Ok(Some(output)) =
                    run_external_backend(backend, file, &span.symbol, max_lines)
                {
                    let selected = output.backend;
                    let truncated = output.truncated;
                    return Ok(BackendSelection {
                        backend: selected,
                        reason: format!("auto selected {selected} for symbol {}", span.symbol),
                        fallback_used: index > 0,
                        external: Some(output),
                        truncated,
                    });
                }
            }
            Ok(BackendSelection {
                backend: "heuristic",
                reason: "auto fell back to heuristic because no external backend succeeded"
                    .to_string(),
                fallback_used: true,
                external: None,
                truncated: local_truncated,
            })
        }
        Backend::AstOutline | Backend::AstBro => {
            run_external_backend(backend, file, &span.symbol, max_lines).map(|external| {
                let external = external.expect("explicit external backend should return output");
                let selected = external.backend;
                let truncated = external.truncated;
                BackendSelection {
                    backend: selected,
                    reason: format!(
                        "explicit backend {selected} selected for symbol {}",
                        span.symbol
                    ),
                    fallback_used: false,
                    external: Some(external),
                    truncated,
                }
            })
        }
    }
}

fn backend_name(backend: Backend) -> &'static str {
    match backend {
        Backend::Heuristic => "heuristic",
        Backend::Auto => "auto",
        Backend::AstOutline => "ast-outline",
        Backend::AstBro => "ast-bro",
    }
}

fn is_external_symbol(symbol: &str) -> bool {
    !symbol.is_empty()
        && !symbol.starts_with('<')
        && !symbol
            .chars()
            .all(|character| character == '`' || character == '~')
}

fn run_external_backend(
    backend: Backend,
    file: &str,
    symbol: &str,
    max_lines: usize,
) -> Result<Option<ExternalSpan>, String> {
    let Some((name, binary)) = external_backend_command(backend) else {
        return Ok(None);
    };

    let mut child = Command::new(binary)
        .args(["show", file, symbol])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("backend {name} is not available in PATH")
            } else {
                format!("backend {name} failed to start: {error}")
            }
        })?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("backend {name} stdout was not captured"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("backend {name} stderr was not captured"))?;
    let stdout_reader = thread::spawn(move || read_limited(stdout, EXTERNAL_STDOUT_LIMIT));
    let stderr_reader = thread::spawn(move || read_limited(stderr, EXTERNAL_STDERR_LIMIT));

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("backend {name} wait failed: {error}"))?
        {
            break status;
        }
        if started.elapsed() >= EXTERNAL_BACKEND_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_limited(stdout_reader, name, "stdout");
            let _ = join_limited(stderr_reader, name, "stderr");
            return Err(format!(
                "backend {name} timed out after {}s",
                EXTERNAL_BACKEND_TIMEOUT.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };

    let stdout = join_limited(stdout_reader, name, "stdout")?;
    let stderr = join_limited(stderr_reader, name, "stderr")?;

    if !status.success() {
        let mut stderr_text = String::from_utf8_lossy(&stderr.bytes).trim().to_string();
        if stderr.truncated {
            stderr_text.push_str(" ... stderr truncated by span");
        }
        return Err(format!(
            "backend {name} failed with exit {}: {}",
            status.code().unwrap_or(1),
            stderr_text
        ));
    }

    let (text, truncated) = cap_external_text(
        &String::from_utf8_lossy(&stdout.bytes),
        max_lines,
        stdout.truncated,
    );
    Ok(Some(ExternalSpan {
        backend: name,
        text,
        truncated,
    }))
}

fn read_limited<R: Read>(mut reader: R, limit: usize) -> io::Result<LimitedOutput> {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut buffer = [0_u8; 16 * 1024];

    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }

        let remaining = limit.saturating_sub(bytes.len());
        if remaining > 0 {
            bytes.extend_from_slice(&buffer[..read.min(remaining)]);
        }
        if read > remaining {
            truncated = true;
        }
    }

    Ok(LimitedOutput { bytes, truncated })
}

fn join_limited(
    handle: thread::JoinHandle<io::Result<LimitedOutput>>,
    backend: &str,
    stream: &str,
) -> Result<LimitedOutput, String> {
    handle
        .join()
        .map_err(|_| format!("backend {backend} {stream} reader panicked"))?
        .map_err(|error| format!("backend {backend} {stream} read failed: {error}"))
}

fn cap_external_text(text: &str, max_lines: usize, output_truncated: bool) -> (String, bool) {
    let mut lines = text.lines().collect::<Vec<_>>();
    if output_truncated {
        let keep = max_lines.saturating_sub(1);
        lines.truncate(keep);
        lines.push("--- truncated by span output cap ---");
        let mut output = lines.join("\n");
        output.push('\n');
        return (output, true);
    }

    if lines.len() <= max_lines {
        return (text.to_string(), false);
    }

    lines.truncate(max_lines);
    if let Some(last) = lines.last_mut() {
        *last = "--- truncated by span --max-lines ---";
    }
    let mut output = lines.join("\n");
    output.push('\n');
    (output, true)
}

fn external_backend_command(backend: Backend) -> Option<(&'static str, &'static str)> {
    match backend {
        Backend::AstOutline => Some(("ast-outline", "ast-outline")),
        Backend::AstBro => Some(("ast-bro", "ast-bro")),
        Backend::Heuristic | Backend::Auto => None,
    }
}

fn markdown_fence_span(lines: &[&str], line: usize) -> Option<Span> {
    let mut open = None::<(usize, char, usize)>;

    for (index, line_text) in lines.iter().enumerate() {
        let fence_line = index + 1;
        if let Some((marker, width)) = markdown_fence_marker(line_text) {
            if let Some((start, open_marker, open_width)) = open {
                if marker == open_marker && width >= open_width {
                    if line >= start && line <= fence_line {
                        return Some(Span {
                            start,
                            end: fence_line,
                            kind: "markdown-fence",
                            symbol: open_marker.to_string().repeat(open_width),
                        });
                    }
                    open = None;
                }
            } else {
                open = Some((fence_line, marker, width));
            }
        }

        if fence_line > line && open.is_none() {
            break;
        }
    }

    if let Some((start, marker, width)) = open {
        if line >= start {
            return Some(Span {
                start,
                end: lines.len(),
                kind: "markdown-fence",
                symbol: marker.to_string().repeat(width),
            });
        }
    }

    None
}

fn markdown_fence_marker(line: &str) -> Option<(char, usize)> {
    let line = line.trim_start();
    let marker = line.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }

    let width = line
        .chars()
        .take_while(|character| *character == marker)
        .count();
    (width >= 3).then_some((marker, width))
}

fn metadata_forward_span(lines: &[&str], line: usize) -> Option<Span> {
    let index = line.checked_sub(1)?;
    if !is_metadata_line(lines.get(index)?) {
        return None;
    }

    let start = metadata_block_start(lines, line);
    let mut symbol_index = index + 1;
    while symbol_index < lines.len() && is_metadata_line(lines[symbol_index]) {
        symbol_index += 1;
    }

    if symbol_index >= lines.len() || !looks_like_symbol(lines[symbol_index]) {
        return None;
    }

    let symbol_start = symbol_index + 1;
    let symbol_line = lines[symbol_index].trim();
    let kind = classify_symbol(symbol_line);
    let symbol = symbol_name(symbol_line);
    let end = brace_span_end(lines, symbol_start)
        .unwrap_or_else(|| indented_span_end(lines, symbol_start));

    Some(Span {
        start,
        end,
        kind,
        symbol,
    })
}

fn metadata_block_start(lines: &[&str], line: usize) -> usize {
    let mut start = line;
    while start > 1 && is_metadata_line(lines[start - 2]) {
        start -= 1;
    }
    start
}

fn attached_metadata_start(lines: &[&str], symbol_start: usize) -> usize {
    metadata_block_start(lines, symbol_start)
}

fn is_metadata_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("#[") || trimmed.starts_with('@')
}

fn syntactic_start_span(lines: &[&str], line: usize) -> Option<Span> {
    let symbol_start = (0..line)
        .rev()
        .find(|&index| looks_like_symbol(lines[index]))?
        + 1;
    let start = attached_metadata_start(lines, symbol_start);
    let symbol_line = lines[symbol_start - 1].trim();
    let kind = classify_symbol(symbol_line);
    let symbol = symbol_name(symbol_line);
    let end = brace_span_end(lines, symbol_start)
        .unwrap_or_else(|| indented_span_end(lines, symbol_start));

    Some(Span {
        start,
        end,
        kind,
        symbol,
    })
}

fn looks_like_symbol(line: &str) -> bool {
    symbol_parts(line).is_some()
}

fn classify_symbol(line: &str) -> &'static str {
    symbol_parts(line).map_or("block", |(kind, _)| kind)
}

fn symbol_name(line: &str) -> String {
    symbol_parts(line).map_or_else(|| "<unknown>".to_string(), |(_, name)| name)
}

fn symbol_parts(line: &str) -> Option<(&'static str, String)> {
    let rest = strip_symbol_prefixes(line.trim_start());

    for (keyword, kind) in [
        ("fn ", "function"),
        ("struct ", "struct"),
        ("enum ", "enum"),
        ("trait ", "trait"),
        ("class ", "class"),
        ("def ", "function"),
        ("func ", "function"),
        ("function ", "function"),
    ] {
        if let Some(name) = rest.strip_prefix(keyword) {
            return Some((kind, first_symbol_token(name)));
        }
    }

    if rest.starts_with("impl ") || rest.starts_with("impl<") {
        return Some(("impl", impl_symbol_name(rest)));
    }

    None
}

fn strip_symbol_prefixes(mut text: &str) -> &str {
    loop {
        if let Some(next) = strip_pub_visibility(text) {
            text = next.trim_start();
            continue;
        }

        let stripped = text
            .strip_prefix("pub ")
            .or_else(|| text.strip_prefix("export "))
            .or_else(|| text.strip_prefix("async "))
            .or_else(|| text.strip_prefix("const "))
            .or_else(|| text.strip_prefix("unsafe "));

        let Some(next) = stripped else {
            return strip_extern_prefix(text);
        };
        text = next.trim_start();
    }
}

fn strip_pub_visibility(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("pub(")?;
    let close = rest.find(')')?;
    Some(&rest[close + 1..])
}

fn strip_extern_prefix(text: &str) -> &str {
    let Some(mut rest) = text.strip_prefix("extern ") else {
        return text;
    };

    rest = rest.trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        if let Some(end_quote) = stripped.find('"') {
            return stripped[end_quote + 1..].trim_start();
        }
    }

    rest
}

fn first_symbol_token(text: &str) -> String {
    text.trim_start()
        .split(|character: char| {
            character == '('
                || character == '<'
                || character == ':'
                || character == '{'
                || character == ';'
                || character.is_whitespace()
        })
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("<unknown>")
        .to_string()
}

fn impl_symbol_name(text: &str) -> String {
    let mut rest = text.strip_prefix("impl").unwrap_or(text).trim_start();

    if rest.starts_with('<') {
        if let Some(end) = matching_angle_end(rest) {
            rest = rest[end + 1..].trim_start();
        }
    }

    if let Some((_, implemented_type)) = rest.rsplit_once(" for ") {
        return type_symbol_token(implemented_type);
    }

    type_symbol_token(rest)
}

fn matching_angle_end(text: &str) -> Option<usize> {
    let mut depth = 0_i32;

    for (index, character) in text.char_indices() {
        match character {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn type_symbol_token(text: &str) -> String {
    let token = text
        .trim_start()
        .split(|character: char| {
            character == '<'
                || character == '{'
                || character == ';'
                || character == '('
                || character.is_whitespace()
        })
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("<unknown>");

    token.rsplit("::").next().unwrap_or(token).to_string()
}

fn brace_span_end(lines: &[&str], start: usize) -> Option<usize> {
    let mut depth = 0_i32;
    let mut saw_open = false;
    let mut block_comment_depth = 0_usize;

    for (offset, line) in lines[start - 1..].iter().enumerate() {
        let (opens, closes) = brace_counts(line, &mut block_comment_depth);
        for _ in 0..opens {
            saw_open = true;
            depth += 1;
        }
        for _ in 0..closes {
            depth -= 1;
        }

        if saw_open && depth <= 0 {
            return Some(start + offset);
        }
    }

    None
}

fn brace_counts(line: &str, block_comment_depth: &mut usize) -> (usize, usize) {
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut opens = 0;
    let mut closes = 0;

    while index < bytes.len() {
        if *block_comment_depth > 0 {
            if bytes.get(index..index + 2) == Some(b"/*") {
                *block_comment_depth += 1;
                index += 2;
            } else if bytes.get(index..index + 2) == Some(b"*/") {
                *block_comment_depth = (*block_comment_depth).saturating_sub(1);
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }

        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => break,
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                *block_comment_depth += 1;
                index += 2;
            }
            b'r' if raw_string_start(bytes, index).is_some() => {
                let (_, hashes) = raw_string_start(bytes, index).expect("checked raw string start");
                index = skip_raw_string(bytes, index, hashes);
            }
            b'"' => index = skip_quoted(bytes, index, b'"'),
            b'\'' => index = skip_quoted(bytes, index, b'\''),
            b'{' => {
                opens += 1;
                index += 1;
            }
            b'}' => {
                closes += 1;
                index += 1;
            }
            _ => index += 1,
        }
    }

    (opens, closes)
}

fn raw_string_start(bytes: &[u8], index: usize) -> Option<(usize, usize)> {
    if bytes.get(index) != Some(&b'r') {
        return None;
    }

    let mut cursor = index + 1;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }

    if bytes.get(cursor) == Some(&b'"') {
        Some((cursor + 1, cursor - index - 1))
    } else {
        None
    }
}

fn skip_raw_string(bytes: &[u8], index: usize, hashes: usize) -> usize {
    let Some((mut cursor, _)) = raw_string_start(bytes, index) else {
        return index + 1;
    };

    while cursor < bytes.len() {
        if bytes[cursor] == b'"'
            && (0..hashes).all(|offset| bytes.get(cursor + 1 + offset) == Some(&b'#'))
        {
            return (cursor + 1 + hashes).min(bytes.len());
        }
        cursor += 1;
    }

    bytes.len()
}

fn skip_quoted(bytes: &[u8], start: usize, quote: u8) -> usize {
    let mut index = start + 1;
    let mut escaped = false;
    while index < bytes.len() {
        if escaped {
            escaped = false;
        } else if bytes[index] == b'\\' {
            escaped = true;
        } else if bytes[index] == quote {
            return index + 1;
        }
        index += 1;
    }

    start + 1
}

fn indented_span_end(lines: &[&str], start: usize) -> usize {
    let base_indent = indent(lines[start - 1]);
    let mut end = start;

    for (offset, line) in lines[start..].iter().enumerate() {
        if !line.trim().is_empty() && indent(line) <= base_indent {
            break;
        }
        end = start + offset + 1;
    }

    end
}

fn indent(line: &str) -> usize {
    let mut columns = 0;
    for character in line.chars() {
        match character {
            ' ' => columns += 1,
            '\t' => columns += 4 - (columns % 4),
            character if character.is_whitespace() => columns += 1,
            _ => break,
        }
    }
    columns
}

fn cap_span(mut semantic: Span, line: usize, max_lines: usize, total_lines: usize) -> SpanView {
    if semantic.end < semantic.start {
        semantic.end = semantic.start;
    }
    semantic.end = semantic.end.min(total_lines);

    let semantic_len = semantic.end - semantic.start + 1;
    if semantic_len <= max_lines {
        return SpanView {
            visible: semantic.clone(),
            semantic,
            truncated: false,
        };
    }

    let target = line.clamp(semantic.start, semantic.end);
    let before = max_lines / 2;
    let mut visible_start = target.saturating_sub(before).max(semantic.start);
    let mut visible_end = (visible_start + max_lines - 1).min(semantic.end);
    if visible_end - visible_start + 1 < max_lines {
        visible_start = visible_end
            .saturating_sub(max_lines - 1)
            .max(semantic.start);
        visible_end = (visible_start + max_lines - 1).min(semantic.end);
    }

    let visible = Span {
        start: visible_start,
        end: visible_end,
        kind: semantic.kind,
        symbol: semantic.symbol.clone(),
    };

    SpanView {
        semantic,
        visible,
        truncated: true,
    }
}

fn print_human(file: &str, line: usize, lines: &[&str], view: &SpanView) {
    println!("file: {file}");
    println!("range: {}..{}", view.semantic.start, view.semantic.end);
    println!(
        "visible range: {}..{}",
        view.visible.start, view.visible.end
    );
    println!("truncated: {}", if view.truncated { "yes" } else { "no" });
    println!("kind: {}", view.semantic.kind);
    println!("symbol: {}", view.semantic.symbol);
    println!();

    if view.visible.start > view.semantic.start {
        println!("  ... truncated before visible range ...");
    }
    for number in view.visible.start..=view.visible.end {
        let marker = if number == line { ">" } else { " " };
        println!("{marker} {number:>4} | {}", lines[number - 1]);
    }
    if view.visible.end < view.semantic.end {
        println!("  ... truncated after visible range ...");
    }
}

fn print_external_human(file: &str, view: &SpanView, external: &ExternalSpan) {
    println!("file: {file}");
    println!("range: {}..{}", view.semantic.start, view.semantic.end);
    println!(
        "visible range: {}..{}",
        view.visible.start, view.visible.end
    );
    println!(
        "truncated: {}",
        if external.truncated { "yes" } else { "no" }
    );
    println!("kind: {}", view.semantic.kind);
    println!("symbol: {}", view.semantic.symbol);
    println!("backend: {}", external.backend);
    println!();
    print!("{}", external.text);
}

fn print_json(
    file: &str,
    line: usize,
    lines: &[&str],
    view: &SpanView,
    selection: &BackendSelection,
) {
    let text = selection.external.as_ref().map_or_else(
        || lines[view.visible.start - 1..view.visible.end].join("\n"),
        |external| external.text.trim_end().to_string(),
    );
    println!(
        "{{\"tool\":\"span\",\"backend\":\"{}\",\"backend_reason\":\"{}\",\"fallback_used\":{},\"truncated\":{},\"file\":\"{}\",\"line\":{},\"range\":[{},{}],\"semantic_range\":[{},{}],\"visible_range\":[{},{}],\"kind\":\"{}\",\"symbol\":\"{}\",\"text\":\"{}\"}}",
        selection.backend,
        json_escape(&selection.reason),
        selection.fallback_used,
        selection.truncated,
        json_escape(file),
        line,
        view.visible.start,
        view.visible.end,
        view.semantic.start,
        view.semantic.end,
        view.visible.start,
        view.visible.end,
        view.semantic.kind,
        json_escape(&view.semantic.symbol),
        json_escape(&text)
    );
}

fn json_optional_string(value: Option<&str>) -> String {
    value.map_or_else(
        || "null".to_string(),
        |value| format!("\"{}\"", json_escape(value)),
    )
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
