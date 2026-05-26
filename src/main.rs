use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

const DEFAULT_MAX_LINES: usize = 80;
const CONTEXT_RADIUS: usize = 20;

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

#[derive(Debug)]
struct Span {
    start: usize,
    end: usize,
    kind: &'static str,
    symbol: String,
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
        let local_truncated = is_span_truncated(&span, args.max_lines);
        let span = cap_span(span, args.max_lines, lines.len());
        let selection =
            select_backend(args.backend, &file, &span, args.max_lines, local_truncated)?;

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
            print_json(&file, line, &lines, &span, &selection);
        } else if let Some(external) = &selection.external {
            print_external_human(&file, &span, external);
        } else {
            print_human(&file, line, &lines, &span);
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
    collect_matches(root, pattern, &mut matches)?;
    matches.sort();
    if matches.is_empty() {
        Err(format!("pattern not found: {pattern}"))
    } else {
        Ok(matches)
    }
}

fn find_symbol(root: &Path, symbol: &str) -> Result<Vec<(String, usize)>, String> {
    let mut matches = Vec::new();
    collect_symbol_matches(root, symbol, &mut matches)?;
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
) -> Result<(), String> {
    if path.is_dir() {
        for entry in sorted_entries(path)? {
            if is_skipped_entry(&entry) {
                continue;
            }
            collect_symbol_matches(&entry, symbol, matches)?;
        }
        return Ok(());
    }

    if !path.is_file() {
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
) -> Result<(), String> {
    if path.is_dir() {
        for entry in sorted_entries(path)? {
            if is_skipped_entry(&entry) {
                continue;
            }
            collect_matches(&entry, pattern, matches)?;
        }
        return Ok(());
    }

    if !path.is_file() {
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
    !symbol.is_empty() && !symbol.starts_with('<') && symbol != "```"
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

    let output = Command::new(binary)
        .args(["show", file, symbol])
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("backend {name} is not available in PATH")
            } else {
                format!("backend {name} failed to start: {error}")
            }
        })?;

    if !output.status.success() {
        return Err(format!(
            "backend {name} failed with exit {}: {}",
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let (text, truncated) = cap_external_text(&String::from_utf8_lossy(&output.stdout), max_lines);
    Ok(Some(ExternalSpan {
        backend: name,
        text,
        truncated,
    }))
}

fn cap_external_text(text: &str, max_lines: usize) -> (String, bool) {
    let mut lines = text.lines().collect::<Vec<_>>();
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
    let mut open_start = None;

    for (index, line_text) in lines.iter().enumerate() {
        let fence_line = index + 1;
        if line_text.trim_start().starts_with("```") {
            if let Some(start) = open_start {
                if line >= start && line <= fence_line {
                    return Some(Span {
                        start,
                        end: fence_line,
                        kind: "markdown-fence",
                        symbol: "```".to_string(),
                    });
                }
                open_start = None;
            } else {
                open_start = Some(fence_line);
            }
        }

        if fence_line > line && open_start.is_none() {
            break;
        }
    }

    None
}

fn syntactic_start_span(lines: &[&str], line: usize) -> Option<Span> {
    let start = (0..line)
        .rev()
        .find(|&index| looks_like_symbol(lines[index]))?
        + 1;
    let symbol_line = lines[start - 1].trim();
    let kind = classify_symbol(symbol_line);
    let symbol = symbol_name(symbol_line);
    let end = brace_span_end(lines, start).unwrap_or_else(|| indented_span_end(lines, start));

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
        let stripped = text
            .strip_prefix("pub(crate) ")
            .or_else(|| text.strip_prefix("pub(super) "))
            .or_else(|| text.strip_prefix("pub(self) "))
            .or_else(|| text.strip_prefix("pub "))
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

    for (offset, line) in lines[start - 1..].iter().enumerate() {
        for character in line.chars() {
            match character {
                '{' => {
                    saw_open = true;
                    depth += 1;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }

        if saw_open && depth <= 0 {
            return Some(start + offset);
        }
    }

    None
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
    line.chars()
        .take_while(|character| character.is_whitespace())
        .count()
}

fn is_span_truncated(span: &Span, max_lines: usize) -> bool {
    span.end >= span.start && span.end - span.start + 1 > max_lines
}

fn cap_span(mut span: Span, max_lines: usize, total_lines: usize) -> Span {
    if span.end < span.start {
        span.end = span.start;
    }

    if span.end - span.start + 1 > max_lines {
        span.end = (span.start + max_lines - 1).min(total_lines);
    }

    span
}

fn print_human(file: &str, line: usize, lines: &[&str], span: &Span) {
    println!("file: {file}");
    println!("range: {}..{}", span.start, span.end);
    println!("kind: {}", span.kind);
    println!("symbol: {}", span.symbol);
    println!();

    for number in span.start..=span.end {
        let marker = if number == line { ">" } else { " " };
        println!("{marker} {number:>4} | {}", lines[number - 1]);
    }
}

fn print_external_human(file: &str, span: &Span, external: &ExternalSpan) {
    println!("file: {file}");
    println!("range: {}..{}", span.start, span.end);
    println!("kind: {}", span.kind);
    println!("symbol: {}", span.symbol);
    println!("backend: {}", external.backend);
    println!();
    print!("{}", external.text);
}

fn print_json(file: &str, line: usize, lines: &[&str], span: &Span, selection: &BackendSelection) {
    let text = selection.external.as_ref().map_or_else(
        || lines[span.start - 1..span.end].join("\n"),
        |external| external.text.trim_end().to_string(),
    );
    println!(
        "{{\"tool\":\"span\",\"backend\":\"{}\",\"backend_reason\":\"{}\",\"fallback_used\":{},\"truncated\":{},\"file\":\"{}\",\"line\":{},\"range\":[{},{}],\"kind\":\"{}\",\"symbol\":\"{}\",\"text\":\"{}\"}}",
        selection.backend,
        json_escape(&selection.reason),
        selection.fallback_used,
        selection.truncated,
        json_escape(file),
        line,
        span.start,
        span.end,
        span.kind,
        json_escape(&span.symbol),
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
