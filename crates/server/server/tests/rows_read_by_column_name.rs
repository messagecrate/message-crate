//! The server reads every column of a database row by its name, never by its
//! place in the `SELECT` list (#2082, #3072). A row read by position takes
//! whatever column sits at that place: reorder the `SELECT`, or add a column
//! before the others, and two columns of the same type swap values with no
//! error.
//!
//! The test walks `src/` and refuses the two positional reads sqlx offers:
//!
//! - `row.try_get(<index>)` on a row from `sqlx::query`.
//! - `sqlx::query_as` into a tuple of more than one column, written as the
//!   tuple or as a `type` alias of one. sqlx maps a tuple by position.
//!
//! So every `query_as` names its row type with a turbofish
//! (`query_as::<_, Row>`, `query_as_with::<_, Row, _>`): a type the test cannot
//! see is a type it cannot check. A struct deriving `sqlx::FromRow` reads its
//! fields by name, and a one-column tuple such as `(String,)` has no order to
//! get wrong. Test code is left out: `tests.rs` files, `tests/` directories
//! and inline `mod tests` blocks.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

fn source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read a source directory") {
        let path = entry.expect("read a directory entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name != "tests") {
                source_files(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && path.file_name().is_some_and(|name| name != "tests.rs")
        {
            out.push(path);
        }
    }
}

/// The file's code before its inline `#[cfg(test)] mod tests { … }` block,
/// with comment lines blanked so prose naming `query_as` is not read as a call.
fn code_without_tests(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut code = String::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]"
            && lines
                .get(i + 1)
                .is_some_and(|next| next.trim_start().starts_with("mod tests {"))
        {
            break;
        }
        if !line.trim_start().starts_with("//") {
            code.push_str(line);
        }
        code.push('\n');
    }
    code
}

/// The text from `open` (an opening bracket) to its matching close,
/// brackets included.
fn bracketed(text: &str, open: usize) -> &str {
    let mut depth = 0usize;
    for (i, c) in text[open..].char_indices() {
        match c {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open..=open + i];
                }
            }
            _ => {}
        }
    }
    &text[open..]
}

/// Whether `ty`, a type starting with `(`, is a tuple of more than one
/// element. `(String,)` is one element; `(i64, String)` is two.
fn is_wide_tuple(ty: &str) -> bool {
    let inner = &ty[1..ty.len() - 1];
    let mut depth = 0usize;
    let mut commas = 0usize;
    for c in inner.chars() {
        match c {
            '(' | '<' | '[' => depth += 1,
            ')' | '>' | ']' => depth -= 1,
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    commas > 1 || (commas == 1 && !inner.trim_end().ends_with(','))
}

/// The names of `type Name = (…);` aliases of a tuple of more than one
/// element.
fn wide_tuple_aliases(code: &str) -> HashSet<String> {
    let mut aliases = HashSet::new();
    for (at, _) in code.match_indices("type ") {
        let rest = &code[at + "type ".len()..];
        let Some((name, after)) = rest.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let after = after.trim_start();
        if after.starts_with('(') {
            let offset = rest.len() - after.len();
            let open = at + "type ".len() + offset;
            if is_wide_tuple(bracketed(code, open)) {
                aliases.insert(name.to_string());
            }
        }
    }
    aliases
}

fn line_of(code: &str, at: usize) -> usize {
    code[..at].matches('\n').count() + 1
}

#[test]
fn every_server_row_is_read_by_column_name() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&src, &mut files);
    files.sort();
    let code: Vec<(String, String)> = files
        .iter()
        .map(|path| {
            let text = fs::read_to_string(path).expect("read a source file");
            let shown = path
                .strip_prefix(&src)
                .unwrap_or(path)
                .display()
                .to_string();
            (shown, code_without_tests(&text))
        })
        .collect();
    let aliases: HashSet<String> = code
        .iter()
        .flat_map(|(_, code)| wide_tuple_aliases(code))
        .collect();

    let mut found = Vec::new();
    for (file, code) in &code {
        for (at, _) in code.match_indices("try_get(") {
            let arg = &code[at + "try_get(".len()..];
            if arg.starts_with(|c: char| c.is_ascii_digit()) {
                found.push(format!("{file}:{}: try_get by index", line_of(code, at)));
            }
        }
        for (at, _) in code.match_indices("query_as") {
            let before = code[..at].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let rest = &code[at + "query_as".len()..];
            let rest = rest.strip_prefix("_with").unwrap_or(rest);
            let Some(turbofish) = rest.strip_prefix("::<") else {
                if rest.starts_with('(') {
                    found.push(format!(
                        "{file}:{}: query_as without a named row type",
                        line_of(code, at)
                    ));
                }
                continue;
            };
            let open = code.len() - turbofish.len() - 1;
            let args = bracketed(code, open);
            let row = args[1..args.len() - 1]
                .split_once(',')
                .map_or("", |(_, row)| row)
                .trim_start();
            let row = if row.starts_with('(') {
                bracketed(row, 0)
            } else {
                row.split(|c: char| c == ',' || c == '>' || c.is_whitespace())
                    .next()
                    .unwrap_or("")
            };
            let wide = if row.starts_with('(') {
                is_wide_tuple(row)
            } else {
                aliases.contains(row.rsplit("::").next().unwrap_or(row))
            };
            if wide {
                found.push(format!(
                    "{file}:{}: query_as into the tuple {row}",
                    line_of(code, at)
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "read these columns by name: alias each column and decode into a struct \
         deriving sqlx::FromRow, or read it with row.try_get(\"<name>\"):\n{}",
        found.join("\n")
    );
}
