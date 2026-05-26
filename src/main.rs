use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

const DEFAULT_MAX_LINES: usize = 80;
const CONTEXT_RADIUS: usize = 20;

#[derive(Debug)]
struct Args {
    target: Target,
    max_lines: usize,
    json: bool,
}

#[derive(Debug)]
enum Target {
    Position(String),
    Contains { pattern: String, root: PathBuf },
    Symbol { symbol: String, root: PathBuf },
}

#[derive(Debug)]
struct Span {
    start: usize,
    end: usize,
    kind: &'static str,
    symbol: String,
}

fn main() {
    let args = match parse_args(env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("span: {message}");
            eprintln!("usage: span [--max-lines N] [--json] FILE:LINE");
            eprintln!("       span [--max-lines N] [--json] --contains PATTERN [PATH]");
            eprintln!("       span [--max-lines N] [--json] --symbol NAME [PATH]");
            process::exit(2);
        }
    };

    match run(&args) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("span: {error}");
            process::exit(1);
        }
    }
}

fn parse_args<I>(input: I) -> Result<Args, String>
where
    I: IntoIterator<Item = String>,
{
    let mut max_lines = DEFAULT_MAX_LINES;
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
            "-h" | "--help" => {
                println!("usage: span [--max-lines N] [--json] FILE:LINE");
                println!("       span [--max-lines N] [--json] --contains PATTERN [PATH]");
                println!("       span [--max-lines N] [--json] --symbol NAME [PATH]");
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
        json,
    })
}

fn run(args: &Args) -> Result<(), String> {
    let (file, line): (String, usize) = match &args.target {
        Target::Position(target) => {
            let (file, line) = parse_target(target)?;
            (file.to_string(), line)
        }
        Target::Contains { pattern, root } => find_contains(root, pattern)?,
        Target::Symbol { symbol, root } => find_symbol(root, symbol)?,
    };
    let text = fs::read_to_string(&file).map_err(|error| format!("{file}: {error}"))?;
    let lines: Vec<&str> = text.lines().collect();

    if line == 0 || line > lines.len() {
        return Err(format!("{file}: line {line} is outside 1..{}", lines.len()));
    }

    let span = find_span(&file, &lines, line);
    let span = cap_span(span, args.max_lines, lines.len());

    if args.json {
        print_json(&file, line, &lines, &span);
    } else {
        print_human(&file, line, &lines, &span);
    }

    Ok(())
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

fn find_contains(root: &Path, pattern: &str) -> Result<(String, usize), String> {
    let mut matches = Vec::new();
    collect_matches(root, pattern, &mut matches)?;
    matches
        .into_iter()
        .next()
        .ok_or_else(|| format!("pattern not found: {pattern}"))
}

fn find_symbol(root: &Path, symbol: &str) -> Result<(String, usize), String> {
    let mut matches = Vec::new();
    collect_symbol_matches(root, symbol, &mut matches)?;
    matches
        .into_iter()
        .next()
        .ok_or_else(|| format!("symbol not found: {symbol}"))
}

fn collect_symbol_matches(
    path: &Path,
    symbol: &str,
    matches: &mut Vec<(String, usize)>,
) -> Result<(), String> {
    if path.is_dir() {
        let entries = fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(name.as_ref(), ".git" | "target" | "node_modules") {
                continue;
            }
            collect_symbol_matches(&entry.path(), symbol, matches)?;
            if !matches.is_empty() {
                break;
            }
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
            break;
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
        let entries = fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(name.as_ref(), ".git" | "target" | "node_modules") {
                continue;
            }
            collect_matches(&entry.path(), pattern, matches)?;
            if !matches.is_empty() {
                break;
            }
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
            break;
        }
    }

    Ok(())
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

fn markdown_fence_span(lines: &[&str], line: usize) -> Option<Span> {
    let mut start = None;
    for index in (0..line).rev() {
        if lines[index].trim_start().starts_with("```") {
            start = Some(index + 1);
            break;
        }
    }

    let start = start?;
    let mut end = None;
    for (index, line_text) in lines.iter().enumerate().skip(line) {
        if line_text.trim_start().starts_with("```") {
            end = Some(index + 1);
            break;
        }
    }

    end.map(|end| Span {
        start,
        end,
        kind: "markdown-fence",
        symbol: "```".to_string(),
    })
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
    let trimmed = line.trim_start();
    trimmed.starts_with("fn ")
        || trimmed.starts_with("pub fn ")
        || trimmed.starts_with("pub(crate) fn ")
        || trimmed.starts_with("impl ")
        || trimmed.starts_with("class ")
        || trimmed.starts_with("def ")
        || trimmed.starts_with("func ")
        || trimmed.starts_with("function ")
        || trimmed.starts_with("export function ")
}

fn classify_symbol(line: &str) -> &'static str {
    if line.starts_with("class ") {
        "class"
    } else if line.starts_with("impl ") {
        "impl"
    } else if line.starts_with("def ") || line.contains(" fn ") || line.starts_with("fn ") {
        "function"
    } else {
        "block"
    }
}

fn symbol_name(line: &str) -> String {
    let cleaned = line
        .replace("pub(crate)", "")
        .replace("pub", "")
        .replace("export", "")
        .replace("function", "")
        .replace("fn", "")
        .replace("def", "")
        .replace("func", "")
        .replace("class", "");

    cleaned
        .trim()
        .split(|character: char| {
            character == '(' || character == '<' || character == ':' || character.is_whitespace()
        })
        .next()
        .unwrap_or("<unknown>")
        .to_string()
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

fn print_json(file: &str, line: usize, lines: &[&str], span: &Span) {
    let text = lines[span.start - 1..span.end].join("\n");
    println!(
        "{{\"tool\":\"span\",\"file\":\"{}\",\"line\":{},\"range\":[{},{}],\"kind\":\"{}\",\"symbol\":\"{}\",\"text\":\"{}\"}}",
        json_escape(file),
        line,
        span.start,
        span.end,
        span.kind,
        json_escape(&span.symbol),
        json_escape(&text)
    );
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
