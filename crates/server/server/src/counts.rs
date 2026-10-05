//! How the server's command and log lines word a count: singular for one and
//! plural for every other count (#1815). The desktop app's lines follow the
//! same rule through `message-crate-core`, which the server does not depend
//! on.

/// `one` for a count of 1, else `many` with `n` in place of `{n}`, so a
/// line whose other words agree with the count, such as `1 conversion
/// failed. That original` against `3 conversions failed. Those originals`,
/// is written whole for each.
pub(crate) fn words(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        many.replace("{n}", &n.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One is the singular line and every other count, 0 included, is the
    /// plural one with the count written in.
    #[test]
    fn one_is_singular_and_every_other_count_is_plural() {
        assert_eq!(words(1, "1 JSONL file", "{n} JSONL files"), "1 JSONL file");
        assert_eq!(words(4, "1 JSONL file", "{n} JSONL files"), "4 JSONL files");
        assert_eq!(words(0, "1 JSONL file", "{n} JSONL files"), "0 JSONL files");
    }
}
