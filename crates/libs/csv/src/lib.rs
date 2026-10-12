//! Shared CSV helpers for reading and writing conversation files.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::io::{Cursor, Write};
use std::path::Path;

/// One attachment object written into `attachments_json`.
#[derive(Debug, Serialize, Deserialize)]
pub struct AttachmentCell {
    /// Shared attachment metadata (serialized inline — same JSON shape as before).
    #[serde(flatten)]
    pub meta: message_ir::AttachmentMeta,
    /// Sticker flag.
    #[serde(default)]
    pub is_sticker: bool,
    /// Transcribed text of the attachment (e.g., OCR of an image or a
    /// voice-note transcript).
    pub transcription: Option<String>,
    /// iMessage sticker effect name.
    pub sticker_effect: Option<String>,
}

impl From<AttachmentCell> for message_ir::IrAttachment {
    fn from(cell: AttachmentCell) -> Self {
        let AttachmentCell {
            meta,
            is_sticker,
            transcription,
            sticker_effect,
        } = cell;
        Self {
            path: meta.path,
            original_name: meta.original_name,
            mime_type: meta.mime_type,
            digest_sha256: meta.digest_sha256,
            is_sticker,
            transcription,
            sticker_effect,
            size_bytes: meta.size_bytes,
            missing_reason: meta.missing_reason,
            bytes: None,
        }
    }
}

/// One participant object written into (and read back from) the CSV
/// `participants_json` cell.
#[derive(Debug, Serialize, Deserialize)]
pub struct ParticipantCell {
    /// Raw identity (phone, email, or other identifier).
    pub identity: String,
    /// Display name; empty string when unknown.
    #[serde(default)]
    pub display_name: String,
}

/// Timestamp formatting (defined in `message-ir`, where the shared
/// projection uses it; re-exported here for existing callers).
pub use message_ir::format_local_ts;

/// Serialize a value for a CSV JSON cell (`null` on failure).
pub fn json_cell(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

/// A CSV reader over a file already read into memory.
pub type CsvBytesReader = csv::Reader<Cursor<Vec<u8>>>;

/// Open a CSV export for reading: the whole file in memory with a UTF-8
/// byte-order mark stripped, a flexible reader over it, and the headers
/// trimmed and lower-cased so [`col`] lookups ignore case.
///
/// # Errors
///
/// Returns an error when the file cannot be read or has no header row.
pub fn open_csv_lowercase(path: &Path) -> anyhow::Result<(CsvBytesReader, Vec<String>)> {
    let mut bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes.drain(..3);
    }
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(Cursor::new(bytes));
    let headers = rdr
        .headers()
        .with_context(|| format!("headers {}", path.display()))?
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();
    Ok((rdr, headers))
}

/// Index of a required CSV header column.
///
/// # Errors
///
/// Returns an error naming the missing column and the headers found.
pub fn col(headers: &[String], name: &str) -> anyhow::Result<usize> {
    headers
        .iter()
        .position(|h| h == name)
        .with_context(|| format!("missing column {name:?} (have {headers:?})"))
}

/// A CSV boolean cell: `1`, `true`, `yes`, or `y` in any case, after trimming.
pub fn parse_bool(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "y"
    )
}

/// What a spreadsheet reads as the start of a formula when a cell begins
/// with it.
const FORMULA_STARTS: [char; 6] = ['=', '+', '-', '@', '\t', '\r'];

/// The `'` a spreadsheet reads as "this cell is text".
const TEXT_MARK: char = '\'';

/// A cell as Message Crate writes it into a CSV file: with a `'` in front
/// when it starts with a character a spreadsheet reads as the start of a
/// formula. Without the `'`, the spreadsheet would run the cell, or drop the
/// `+` of a phone number. A cell that already starts with `'`s and then one
/// of those characters gets one more `'`, so the one [`read_cell`] takes
/// off leaves the cell as it was.
pub fn written_cell(cell: &str) -> Cow<'_, str> {
    if cell
        .trim_start_matches(TEXT_MARK)
        .starts_with(FORMULA_STARTS)
    {
        format!("{TEXT_MARK}{cell}").into()
    } else {
        cell.into()
    }
}

/// A cell as Message Crate reads it from a CSV file: one `'` taken off when
/// one of the characters a spreadsheet reads as a formula follows it, which
/// undoes [`written_cell`]. A spreadsheet can keep that `'` when it saves or
/// drop it, and both read the same, for every cell that did not itself start
/// with `'` and one of those characters. Such a cell, `'=x`, is written
/// `''=x`; a spreadsheet that drops the outer `'` saves `'=x`, which reads
/// back as `=x`.
pub fn read_cell(cell: &str) -> &str {
    match cell.strip_prefix(TEXT_MARK) {
        Some(rest)
            if rest
                .trim_start_matches(TEXT_MARK)
                .starts_with(FORMULA_STARTS) =>
        {
            rest
        }
        _ => cell,
    }
}

/// Write one CSV row with every cell as [`written_cell`] writes it.
///
/// # Errors
///
/// Returns the writer's error when the row cannot be written.
pub fn write_guarded_record<W: Write>(
    wtr: &mut csv::Writer<W>,
    cells: impl IntoIterator<Item = impl AsRef<str>>,
) -> csv::Result<()> {
    for cell in cells {
        wtr.write_field(written_cell(cell.as_ref()).as_bytes())?;
    }
    wtr.write_record(None::<&[u8]>)
}

/// Trimmed value of one CSV cell (empty string when missing).
pub fn field(rec: &csv::StringRecord, idx: usize) -> String {
    rec.get(idx).unwrap_or("").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::{col, field, json_cell, parse_bool, read_cell, write_guarded_record, written_cell};

    /// Every character a spreadsheet reads as the start of a formula gets the
    /// `'`, and the read takes exactly that `'` off again.
    #[test]
    fn every_formula_start_is_quoted_and_read_back() {
        for cell in ["=1", "+1", "-1", "@a", "\ta", "\ra", "'=1", "''+1"] {
            let written = written_cell(cell);
            assert_eq!(written, format!("'{cell}"), "{cell:?}");
            assert_eq!(read_cell(&written), cell, "{cell:?}");
        }
        for cell in ["", "Ada", "'Twas", "a=1", "15555550100", r#"[{"a":1}]"#] {
            assert_eq!(written_cell(cell), cell, "{cell:?}");
            assert_eq!(read_cell(cell), cell, "{cell:?}");
        }
    }

    #[test]
    fn write_guarded_record_quotes_every_formula_cell_of_a_row() {
        let mut wtr = csv::Writer::from_writer(Vec::new());
        write_guarded_record(&mut wtr, ["=1+1", "plain", "+15551234567"]).unwrap();
        let text = String::from_utf8(wtr.into_inner().unwrap()).unwrap();
        assert_eq!(text, "'=1+1,plain,'+15551234567\n");
    }

    #[test]
    fn col_finds_a_column_by_name_and_names_a_missing_one() {
        let headers = vec!["guid".to_string(), "text".to_string(), "date".to_string()];
        assert_eq!(col(&headers, "text").unwrap(), 1);
        assert_eq!(col(&headers, "date").unwrap(), 2);
        let err = col(&headers, "subject").unwrap_err().to_string();
        assert!(err.contains("\"subject\""), "{err}");
    }

    #[test]
    fn parse_bool_reads_the_true_spellings_and_nothing_else() {
        for raw in ["1", "true", " TRUE ", "yes", "Y"] {
            assert!(parse_bool(raw), "{raw}");
        }
        for raw in ["0", "false", "no", "", "2"] {
            assert!(!parse_bool(raw), "{raw}");
        }
    }

    #[test]
    fn field_trims_a_cell_and_reads_a_missing_one_as_empty() {
        let rec = csv::StringRecord::from(vec![" hello ", "x"]);
        assert_eq!(field(&rec, 0), "hello");
        assert_eq!(field(&rec, 5), "");
    }

    #[test]
    fn json_cell_writes_json() {
        assert_eq!(json_cell(&vec!["a", "b"]), r#"["a","b"]"#);
    }
}
