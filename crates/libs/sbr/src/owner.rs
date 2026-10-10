//! The owner's phone numbers, inferred from the sent MMS in a backup.

use anyhow::{Context, Result, bail};
use quick_xml::{Reader, events::Event};
use std::collections::HashMap;
use std::path::Path;

use message_ir::IdentityType;

use crate::addresses::{MMS_ADDR_FROM, address_handle};
use crate::mms_box::MmsBox;
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
                    "mms" => {
                        in_sent = MmsBox::parse(get(&attrs(&e, &mut 0), "msg_box"))
                            .is_some_and(MmsBox::is_sent);
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_owner_from_nested_addr() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smses.xml");
        std::fs::write(&path, r#"<smses><mms msg_box="2"><parts/><addrs><addr address="+15555550100" type="137"/></addrs></mms></smses>"#).unwrap();
        assert_eq!(infer_owner_phones(&path).unwrap(), vec!["+15555550100"]);
    }

    #[test]
    fn an_owner_outside_the_us_is_inferred_with_its_country() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smses.xml");
        std::fs::write(&path, r#"<smses><mms msg_box="2"><parts/><addrs><addr address="+447700900456" type="137"/></addrs></mms></smses>"#).unwrap();
        assert_eq!(infer_owner_phones(&path).unwrap(), vec!["+447700900456"]);
    }
}
