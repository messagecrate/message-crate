//! The server reads every column of a database row by its name, never by its
//! place in the `SELECT` list (#2082, #3072). A row read by position takes
//! whatever column sits at that place: reorder the `SELECT`, or add a column
//! before the others, and two columns of the same type swap values with no
//! error.
//!
//! The test walks `src/` and refuses the positional reads sqlx offers:
//!
//! - `try_get`, `get` and their `_unchecked` forms given a column index. `get`
//!   is checked only in a file that names `sqlx::Row`, since a slice's `get`
//!   looks the same.
//! - `sqlx::query_as` into a tuple of more than one column, written as the
//!   tuple or as a `type` alias of one. sqlx maps a tuple by position.
//! - `#[derive(sqlx::FromRow)]` on a tuple struct, which sqlx also maps by
//!   position.
//!
//! So every `query_as` names its row type with a turbofish
//! (`query_as::<_, Row>`, `query_as_with::<_, Row, _>`): a type the test cannot
//! see is a type it cannot check. A struct deriving `sqlx::FromRow` reads its
//! fields by name, and a one-column tuple such as `(String,)` has no order to
//! get wrong. Test code is left out: `tests.rs` files, `tests/` directories
//! and inline `#[cfg(test)] mod tests { … }` blocks.

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

/// `text` with its comments removed and the contents of its string, byte
/// string, raw string and char literals emptied, so a brace, a bracket or
/// the word `query_as` inside one is not read as code. Newlines are kept, so
/// line numbers stay those of the file.
fn code_only(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let starts_word = i == 0 || !(at(i - 1).is_alphanumeric() || at(i - 1) == '_');
        if c == '/' && at(i + 1) == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && at(i + 1) == '*' {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    if chars[i] == '\n' {
                        out.push('\n');
                    }
                    i += 1;
                }
            }
        } else if let Some((opening, hashes)) = starts_word
            .then(|| raw_string_opening(&chars[i..]))
            .flatten()
        {
            i += opening;
            out.push_str("\"\"");
            while i < chars.len() {
                if chars[i] == '"' && (1..=hashes).all(|k| at(i + k) == '#') {
                    i += hashes;
                    break;
                }
                if chars[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i += 1;
        } else if c == '"' || (starts_word && c == 'b' && at(i + 1) == '"') {
            i += if c == 'b' { 2 } else { 1 };
            out.push_str("\"\"");
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                if at(i) == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            i += 1;
        } else if c == '\'' && (at(i + 1) == '\\' || at(i + 2) == '\'') {
            // A char literal; a `'` followed by anything else is a lifetime.
            i += 1;
            if at(i) == '\\' {
                i += 1;
            }
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                i += 1;
            }
            i += 1;
            out.push_str("''");
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// The length of the opening of a raw string (`r"`, `r#"`, `br##"` and so
/// on) that `chars` starts with, and its number of `#`s.
fn raw_string_opening(chars: &[char]) -> Option<(usize, usize)> {
    let mut i = match chars {
        ['r', ..] => 1,
        ['b', 'r', ..] => 2,
        _ => return None,
    };
    let mut hashes = 0;
    while chars.get(i) == Some(&'#') {
        hashes += 1;
        i += 1;
    }
    (chars.get(i) == Some(&'"')).then_some((i + 1, hashes))
}

/// The code of `text` ([`code_only`]) without its inline
/// `#[cfg(test)] mod tests { … }` blocks. Every line is kept, blank where it
/// was left out, so line numbers stay those of the file.
fn code_without_tests(text: &str) -> String {
    let masked = code_only(text);
    let lines: Vec<&str> = masked.lines().collect();
    let mut code = String::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "#[cfg(test)]" {
            let mut next = i + 1;
            while lines
                .get(next)
                .is_some_and(|line| line.trim_start().starts_with("#["))
            {
                next += 1;
            }
            if lines
                .get(next)
                .is_some_and(|line| line.trim_start().starts_with("mod tests {"))
            {
                let mut depth = 0i64;
                let mut end = next;
                while end < lines.len() {
                    depth += braces(lines[end]);
                    if depth <= 0 {
                        break;
                    }
                    end += 1;
                }
                assert!(
                    end < lines.len(),
                    "the test module at line {} never closes",
                    next + 1
                );
                for _ in i..=end {
                    code.push('\n');
                }
                i = end + 1;
                continue;
            }
        }
        code.push_str(lines[i]);
        code.push('\n');
        i += 1;
    }
    code
}

/// Opening braces less closing braces on `line`.
fn braces(line: &str) -> i64 {
    line.chars()
        .map(|c| match c {
            '{' => 1,
            '}' => -1,
            _ => 0,
        })
        .sum()
}

/// The text from `open` (an opening bracket) to its matching close,
/// brackets included. `->` and `=>` are not brackets.
fn bracketed(text: &str, open: usize) -> &str {
    let mut depth = 0i64;
    let mut previous = ' ';
    for (i, c) in text[open..].char_indices() {
        match c {
            '(' | '<' | '[' => depth += 1,
            '>' if previous == '-' || previous == '=' => {}
            ')' | '>' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open..=open + i];
                }
            }
            _ => {}
        }
        previous = c;
    }
    &text[open..]
}

/// The top-level, comma-separated items between the outer brackets of
/// `group`, trimmed.
fn items(group: &str) -> Vec<&str> {
    let mut inside = group.chars();
    inside.next();
    inside.next_back();
    let inner = inside.as_str();
    let mut out = Vec::new();
    let mut depth = 0i64;
    let mut previous = ' ';
    let mut start = 0;
    for (i, c) in inner.char_indices() {
        match c {
            '(' | '<' | '[' => depth += 1,
            '>' if previous == '-' || previous == '=' => {}
            ')' | '>' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(inner[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        previous = c;
    }
    let last = inner[start..].trim();
    if !last.is_empty() {
        out.push(last);
    }
    out
}

/// Whether `ty`, a type starting with `(`, is a tuple of more than one
/// element. `(String,)` is one element; `(i64, String)` is two.
fn is_wide_tuple(ty: &str) -> bool {
    items(ty).len() > 1
}

/// A type's name without its path and generic arguments: `Pair` for
/// `crate::db::Pair<T>`.
fn bare_name(ty: &str) -> &str {
    let ty = ty.split('<').next().unwrap_or(ty).trim();
    ty.rsplit("::").next().unwrap_or(ty)
}

/// The names of `type Name = (…);` aliases of a tuple of more than one
/// element.
fn wide_tuple_aliases(code: &str) -> HashSet<String> {
    let mut aliases = HashSet::new();
    for (at, _) in code.match_indices("type ") {
        if code[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        let rest = &code[at + "type ".len()..];
        let Some((name, after)) = rest.split_once('=') else {
            continue;
        };
        let name = bare_name(name);
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let after = after.trim_start();
        if after.starts_with('(') {
            let open = code.len() - after.len();
            if is_wide_tuple(bracketed(code, open)) {
                aliases.insert(name.to_string());
            }
        }
    }
    aliases
}

/// The places a `#[derive(…FromRow…)]` sits on a tuple struct.
fn from_row_tuple_structs(code: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for (at, _) in code.match_indices("#[derive(") {
        let attribute = bracketed(code, at + 1);
        if !attribute.contains("FromRow") {
            continue;
        }
        // Skip any further attributes and doc comments to the item.
        let mut rest = &code[at + 1 + attribute.len()..];
        loop {
            rest = rest.trim_start();
            if rest.starts_with("#[") {
                let skip = bracketed(rest, 1).len() + 1;
                rest = &rest[skip..];
            } else {
                break;
            }
        }
        let rest = rest
            .strip_prefix("pub(crate) ")
            .or_else(|| rest.strip_prefix("pub(super) "))
            .or_else(|| rest.strip_prefix("pub "))
            .unwrap_or(rest);
        let Some(rest) = rest.strip_prefix("struct ") else {
            continue;
        };
        let name_end = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        let mut after = rest[name_end..].trim_start();
        if after.starts_with('<') {
            after = after[bracketed(after, 0).len()..].trim_start();
        }
        if after.starts_with('(') {
            found.push(at);
        }
    }
    found
}

/// Whether the file brings sqlx's `Row` trait into reach, so a `get` in it
/// may be a row's: it names `sqlx::Row`, or a `use sqlx::…;` statement, on
/// one line or several, names `Row`.
fn names_sqlx_row(code: &str) -> bool {
    code.contains("sqlx::Row")
        || code.match_indices("use sqlx::").any(|(at, _)| {
            let statement = code[at..].split(';').next().unwrap_or("");
            statement
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .any(|word| word == "Row")
        })
}

/// The places a column is read by index with `method` (and its turbofish
/// form): an argument that is a bare number.
fn index_reads(code: &str, method: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for (at, _) in code.match_indices(method) {
        let before = code[..at].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let mut rest = &code[at + method.len()..];
        if let Some(turbofish) = rest.strip_prefix("::<") {
            let open = code.len() - turbofish.len() - 1;
            rest = &code[open + bracketed(code, open).len()..];
        }
        if !rest.starts_with('(') {
            continue;
        }
        let open = code.len() - rest.len();
        let by_index = items(bracketed(code, open)).iter().any(|arg| {
            let number = arg.trim_end_matches("usize");
            number.starts_with(|c: char| c.is_ascii_digit())
                && number.chars().all(|c| c.is_ascii_digit() || c == '_')
        });
        if by_index {
            found.push(at);
        }
    }
    found
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
        let mut methods = vec!["try_get", "try_get_unchecked"];
        if names_sqlx_row(code) {
            methods.extend(["get", "get_unchecked"]);
        }
        for method in methods {
            for at in index_reads(code, method) {
                found.push(format!("{file}:{}: {method} by index", line_of(code, at)));
            }
        }
        for at in from_row_tuple_structs(code) {
            found.push(format!(
                "{file}:{}: FromRow on a tuple struct",
                line_of(code, at)
            ));
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
            let arguments = items(bracketed(code, open));
            let row = arguments.get(1).copied().unwrap_or("");
            let wide = if row.starts_with('(') {
                is_wide_tuple(row)
            } else {
                aliases.contains(bare_name(row))
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
         with named fields deriving sqlx::FromRow, or read it with \
         row.try_get(\"<name>\"):\n{}",
        found.join("\n")
    );
}
