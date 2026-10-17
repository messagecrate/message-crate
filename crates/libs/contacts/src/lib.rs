//! A vCard (VCF) reader.
//!
//! Message Crate does not load a vCard as an address book: its address book
//! is its own CSV, which the server writes and loads. The server reads a
//! vCard with [`parse_vcf`] only to convert it to that CSV, for a person to
//! edit and then load.

mod vcf;

pub use vcf::{UnknownCharset, VcfCard, parse_vcf};
