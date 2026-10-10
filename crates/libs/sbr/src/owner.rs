//! The owner's phone numbers, inferred from the sent MMS in a backup.

use anyhow::{Context, Result, bail};
use quick_xml::{Reader, events::Event};
use std::collections::HashMap;
use std::path::Path;

use message_ir::IdentityType;

use crate::addresses::{MMS_ADDR_FROM, address_handle};
use crate::read::MMS_BOX_SENT;
use crate::xml::{attrs, get};

/// Infer owner phones from nested `<addr type="137">` elements in sent MMS.
///
/// # Errors
///
/// Returns an error when the file cannot be opened or parsed.
pub fn infer_owner_phones(path: &Path) -> Result<Vec<String>> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut xml = Reader::from_reader(std::io::BufReader::new(file));
    let (mut buf, mut in_sent, mut counts) = (Vec::new(), false, HashMap::<String, u64>::new());
    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                match e.name().as_ref().to_ascii_lowercase().as_str() {
                    "mms" => in_sent = get(&attrs(&e, &mut 0), "msg_box").trim() == MMS_BOX_SENT,
                    "addr" if in_sent => {
                        let a = attrs(&e, &mut 0);
                        if get(&a, "type").trim() == MMS_ADDR_FROM {
                            let raw = get(&a, "address");
                            if let Some(owner) =
                                address_handle(raw).filter(|h| h.kind() == IdentityType::Phone)
                            {
                                *counts.entry(owner.into_key()).or_default() += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) if e.name().as_ref().eq_ignore_ascii_case("mms") => in_sent = false,
            Ok(Event::Eof) => break,
            Err(error) => bail!("parse {}: {error}", path.display()),
            _ => {}
        }
        buf.clear();
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(ranked.into_iter().map(|(phone, _)| phone).collect())
}
