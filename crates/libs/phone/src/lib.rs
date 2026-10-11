//! Shared phone-number parsing for message converters.
//!
//! [`key_typed_handle`] is the one key a handle is stored and matched under:
//! by the server, by the contacts book, and by [`OwnerHandleSet`].
//! [`Handle::parse`] is where an exporter classifies an address before it
//! keys it: a phone number, an email address, or a sender name.
//!
//! A number carries its country only when that is certain: written with its
//! `+` code, or written without it in a country a person or the source
//! stated ([`Country`]). Nothing here reads a country from the digits, so a
//! number written `07700 900123` with no country stated keeps its digits
//! (#1676).

mod countries;

use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use countries::{COUNTRIES, Country, country};

/// Kind of a participant identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentityType {
    /// Telephone number.
    Phone,
    /// Email address.
    Email,
    /// App username (e.g. Telegram `@user`).
    Username,
    /// Any identity that is not phone, email, or username.
    Other,
}

impl IdentityType {
    /// Lowercase storage id (`phone` / `email` / `username` / `other`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Phone => "phone",
            Self::Email => "email",
            Self::Username => "username",
            Self::Other => "other",
        }
    }

    /// Parse a storage id; unknown values map to `Other`.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "phone" => Self::Phone,
            "email" => Self::Email,
            "username" => Self::Username,
            _ => Self::Other,
        }
    }
}

/// Minimum digit length after stripping formatting.
///
/// Allows 4–6 digit short codes (carrier/bank SMS, e.g. AT&T `7535`).
/// Rejects junk like `"4"` or `"06"`.
const MIN_PHONE_DIGITS: usize = 4;

/// Which rules write a number in its `+` form, for [`normalize_checked`] and
/// [`normalize_guarded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneRegion {
    /// The number was written with a `+` before its first digit, so it names
    /// its own country.
    International,
    /// The number was written without its `+` code, in the national form of
    /// this country, which a person or the source stated.
    Country(&'static Country),
    /// The number was written without its `+` code, and nothing says which
    /// country it belongs to.
    Unknown,
}

impl PhoneRegion {
    /// Region for a raw value: a value with a `+` before its first digit
    /// names its country, so international rules apply (`+65 5555 0100`,
    /// `(+44) 7700 900123`, `tel:+447700900123`). Anything else is in the
    /// national form of `country`, or of no known country when `country` is
    /// `None`. A number without `+` is never read as a US number by default
    /// (#1676).
    #[must_use]
    pub fn for_raw(raw: &str, country: Option<&'static Country>) -> Self {
        if written_with_plus(raw) {
            Self::International
        } else {
            country.map_or(Self::Unknown, Self::Country)
        }
    }
}

/// Whether a `+` comes before the first digit of `raw`.
fn written_with_plus(raw: &str) -> bool {
    raw.split(|c: char| c.is_ascii_digit())
        .next()
        .is_some_and(|s| s.contains('+'))
}

/// Strip non-digits and a leading US country code `1`.
/// Returns `None` when fewer than the minimum digit count (4) remain.
pub fn sanitize_number(num: &str) -> Option<String> {
    if num.is_empty() {
        return None;
    }
    let mut digits: String = num.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 11 && digits.starts_with('1') {
        digits = digits[1..].to_string();
    }
    if digits.len() < MIN_PHONE_DIGITS {
        None
    } else {
        Some(digits)
    }
}

/// Punctuation a written phone number may carry between or around its digits.
///
/// `/` is deliberately absent: it separates two numbers ("home / cell") far
/// more often than it appears inside one.
const PHONE_PUNCTUATION: [char; 7] = ['+', '-', '(', ')', '.', ' ', '\t'];

/// The most digits a phone number can have, from ITU-T E.164.
const MAX_PHONE_DIGITS: usize = 15;

/// Digits of a value that is *written as* a phone number, rather than a value
/// that merely contains enough digits.
///
/// [`sanitize_number`] keeps every ASCII digit it finds and then asks only how
/// many there are, so `met in 2019 at 42 Acacia Avenue` sanitizes to `201942`.
/// That is the right rule for a field known to hold a number and allowed to
/// carry stray formatting; it is the wrong rule for a field that may hold
/// anything, or that may hold two numbers with nothing between them.
///
/// Two conditions are added. The value must be nothing but digits and phone
/// punctuation, so prose is rejected before any digits are collected. And the
/// digits that remain must number no more than [`MAX_PHONE_DIGITS`], so two
/// numbers run together are rejected rather than concatenated into a handle
/// that matches nothing.
///
/// An extension suffix (`555-0123 x99`) is prose by the first rule and is
/// rejected whole, which is deliberate: gluing an extension onto a number
/// produces a handle that matches nothing either.
///
/// Returns `None` when the value carries any other character, when the digits
/// are too many, or when [`sanitize_number`] rejects what remains.
#[must_use]
pub fn sanitize_phone_shaped(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_digit() || PHONE_PUNCTUATION.contains(&c))
    {
        return None;
    }
    let digits = sanitize_number(value)?;
    (digits.len() <= MAX_PHONE_DIGITS).then_some(digits)
}

/// ITU-T E.164 country calling codes, longest-first for greedy prefix match.
const COUNTRY_CALLING_CODES: &[&str] = &[
    "211", "212", "213", "216", "218", "220", "221", "222", "223", "224", "225", "226", "227",
    "228", "229", "230", "231", "232", "233", "234", "235", "236", "237", "238", "239", "240",
    "241", "242", "243", "244", "245", "246", "248", "249", "250", "251", "252", "253", "254",
    "255", "256", "257", "258", "260", "261", "262", "263", "264", "265", "266", "267", "268",
    "269", "290", "291", "297", "298", "299", "350", "351", "352", "353", "354", "355", "356",
    "357", "358", "359", "370", "371", "372", "373", "374", "375", "376", "377", "378", "380",
    "381", "382", "383", "385", "386", "387", "389", "420", "421", "423", "500", "501", "502",
    "503", "504", "505", "506", "507", "508", "509", "590", "591", "592", "593", "594", "595",
    "596", "597", "598", "599", "670", "672", "673", "674", "675", "676", "677", "678", "679",
    "680", "681", "682", "683", "685", "686", "687", "688", "689", "690", "691", "692", "850",
    "852", "853", "855", "856", "880", "886", "960", "961", "962", "963", "964", "965", "966",
    "967", "968", "970", "971", "972", "973", "974", "975", "976", "977", "992", "993", "994",
    "995", "996", "998", "20", "27", "30", "31", "32", "33", "34", "36", "39", "40", "41", "43",
    "44", "45", "46", "47", "48", "49", "51", "52", "53", "54", "55", "56", "57", "58", "60", "61",
    "62", "63", "64", "65", "66", "81", "82", "84", "86", "90", "91", "92", "93", "94", "95", "98",
    "1", "7",
];

/// Minimum national-number digits required after peeling a country calling code.
/// Keeps short codes (4–6 digits) from being misread as country + stub, and
/// is the shortest number written without its `+` that has a `+` form once
/// its country is known.
pub const MIN_NATIONAL_DIGITS: usize = 7;

/// Split digits into `(country_calling_code, national_number)`.
///
/// When `had_plus` is true, uses longest-match ITU calling codes. Without `+`, only
/// recognizes the NANP leading `1` on 11-digit numbers. Returns `("", digits)` when
/// no country code can be identified safely.
pub fn split_country_calling_code(digits: &str, had_plus: bool) -> (&str, &str) {
    if had_plus {
        for cc in COUNTRY_CALLING_CODES {
            if digits.starts_with(cc) && digits.len() - cc.len() >= MIN_NATIONAL_DIGITS {
                return (cc, &digits[cc.len()..]);
            }
        }
        return ("", digits);
    }
    if digits.len() == 11 && digits.starts_with('1') {
        return ("1", &digits[1..]);
    }
    ("", digits)
}

/// E.164 when the parse is unambiguous for `region`, else the human-readable
/// reason it is not.
///
/// Unlike [`sanitize_number`], this does **not** accept short codes or
/// ambiguous lengths. [`normalize_guarded`] calls it and keeps the error
/// string as the review note the server shows.
///
/// # Errors
///
/// Returns the reason the value is not certain E.164: empty, no digits, no
/// country known, the wrong digit count for the country, a missing `+` in
/// international mode, or a country code starting with 0 (0 is the trunk
/// prefix, so `+020…` would be fabricated).
pub fn normalize_checked(raw: &str, region: PhoneRegion) -> Result<String, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("empty phone".into());
    }
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return Err("no digits".into());
    }
    // A `+` names the number's own country, whatever country is stated.
    let region = if written_with_plus(raw) {
        PhoneRegion::International
    } else {
        region
    };
    match region {
        PhoneRegion::Unknown => Err("written without its country code, in no known country".into()),
        PhoneRegion::Country(country) if country.calling_code == "1" => {
            if let Some(international) = after_international_prefix(&digits, country) {
                dialled_abroad(international, country)
            } else if digits.len() == 10 {
                Ok(format!("+1{digits}"))
            } else if digits.len() == 11 && digits.starts_with('1') {
                Ok(format!("+{digits}"))
            } else {
                Err(format!(
                    "a number in {} has 10 digits, or 11 starting with 1",
                    country.code
                ))
            }
        }
        PhoneRegion::Country(country) => in_country(&digits, country),
        PhoneRegion::International => {
            if written_with_plus(raw) {
                written_abroad(&digits)
            } else {
                Err("international mode requires a leading +".into())
            }
        }
    }
}

/// The `+` form of `digits`, a number written without its `+` in `country`
/// (not a `+1` country, which [`normalize_checked`] writes by its own rule).
///
/// The country's own international prefix before the digits means what
/// follows is a calling code and number ([`dialled_abroad`]): `00 44 7700
/// 900123` in the United Kingdom, `0011 44 …` in Australia, `8 10 44 …` in
/// Russia. Otherwise the country's trunk prefix is dropped (`07700 900123` in
/// the United Kingdom).
///
/// A number that starts with the country's own calling code may have been
/// written in full without its `+` ([`with_calling_code`]). It is read so
/// when the trunk prefix is kept after the code (`44 (0)7700 900123`), or
/// when what follows the code is as long as the country's numbers are and
/// the digits as a whole are not (`447700900123`). When both readings have a
/// valid length, as `49 151 23456789` has in Germany, the number is refused
/// rather than guessed: it could be either. When only the whole is valid,
/// it is the national number: `55 99123 4567` in Brazil has the area code
/// 55. Anything else is the national number.
fn in_country(digits: &str, country: &Country) -> Result<String, String> {
    if let Some(international) = after_international_prefix(digits, country) {
        return dialled_abroad(international, country);
    }
    let trunk = country.trunk_prefix;
    let national = if !trunk.is_empty() && digits.starts_with(trunk) {
        &digits[trunk.len()..]
    } else {
        with_calling_code(digits, country)?.unwrap_or(digits)
    };
    if national.len() < MIN_NATIONAL_DIGITS {
        Err(format!("too few digits for a number in {}", country.code))
    } else if country.calling_code.len() + national.len() > MAX_PHONE_DIGITS {
        Err(format!("too many digits for a number in {}", country.code))
    } else {
        Ok(format!("+{}{national}", country.calling_code))
    }
}

/// The national number of `digits` read as written with `country`'s calling
/// code and no `+`, when that is the reading: `None` when it is not, and a
/// refusal when the digits read as well either way ([`in_country`]).
fn with_calling_code<'a>(digits: &'a str, country: &Country) -> Result<Option<&'a str>, String> {
    let national_length =
        |n: &str| u8::try_from(n.len()).is_ok_and(|len| country.national_lengths.contains(&len));
    let Some(rest) = digits.strip_prefix(country.calling_code) else {
        return Ok(None);
    };
    let trunk = country.trunk_prefix;
    if let Some(after_trunk) = rest.strip_prefix(trunk).filter(|_| !trunk.is_empty()) {
        // `44 (0)7700 900123`: the trunk prefix kept after the code says the
        // number was written in full.
        return Ok(national_length(after_trunk).then_some(after_trunk));
    }
    match (national_length(rest), national_length(digits)) {
        (true, false) => Ok(Some(rest)),
        (true, true) => Err(format!(
            "{digits} could be +{cc}{digits} or +{cc}{rest}; pick its country or write it with +",
            cc = country.calling_code
        )),
        (false, _) => Ok(None),
    }
}

/// The digits after `country`'s international prefix, when they start with
/// one ([`Country::international_prefix`]).
fn after_international_prefix<'a>(digits: &'a str, country: &Country) -> Option<&'a str> {
    static PREFIXES: LazyLock<HashMap<&'static str, Regex>> = LazyLock::new(|| {
        COUNTRIES
            .iter()
            .map(|c| {
                let pattern = format!("^(?:{})", c.international_prefix);
                let prefix = Regex::new(&pattern).expect("libphonenumber's prefix is a regex");
                (c.code, prefix)
            })
            .collect()
    });
    let found = PREFIXES.get(country.code)?.find(digits)?;
    Some(&digits[found.end()..])
}

/// The `+` form of `digits`, dialled after `country`'s international
/// prefix. Digits that start with no calling code are refused, because the
/// prefix was then most likely not one: a wrong guess would give a key that
/// names nobody.
fn dialled_abroad(digits: &str, country: &Country) -> Result<String, String> {
    if !COUNTRIES.iter().any(|c| digits.starts_with(c.calling_code)) {
        return Err(format!(
            "not a number {} can read: what follows its international prefix starts \
             with no calling code",
            country.code
        ));
    }
    written_abroad(digits)
}

/// The `+` form of `digits`, the calling code and number written after a
/// `+` or an international prefix.
fn written_abroad(digits: &str) -> Result<String, String> {
    if digits.starts_with('0') {
        // E.164 country codes never start with 0 (0 is the trunk prefix),
        // so a `+020…` value is fabricated, not certain.
        Err("international country code cannot start with 0".into())
    } else if (8..=MAX_PHONE_DIGITS).contains(&digits.len()) {
        Ok(format!("+{digits}"))
    } else {
        Err("international needs 8–15 digits after +".into())
    }
}

/// Result of [`normalize_guarded`]: the E.164 value when certain, plus a
/// review note when the value is ambiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedNormalize {
    /// The phone value to store: E.164 (`+44…`) when the parse was certain,
    /// otherwise the digits as written, without a `+`.
    pub normalized: String,
    /// `Some(reason)` when the value was ambiguous and stored digits-as-is;
    /// `None` when certain.
    pub note: Option<String>,
}

/// E.164 when unambiguous for `region`, else digits-as-is plus a note.
///
/// Rewrite to E.164 only when the parse is certain. Otherwise store the
/// digits as written, without adding a `+` prefix (a trunk-zero national
/// number like `020 7946 0000` would otherwise become the invalid
/// `+02079460000`), and attach a human-readable reason so the server can
/// show it for review.
#[must_use]
pub fn normalize_guarded(raw: &str, region: PhoneRegion) -> GuardedNormalize {
    match normalize_checked(raw, region) {
        Ok(e164) => GuardedNormalize {
            normalized: e164,
            note: None,
        },
        Err(reason) => GuardedNormalize {
            normalized: digits_as_written(raw).unwrap_or_default(),
            note: Some(reason),
        },
    }
}

/// Every digit of `raw`, in order, with nothing taken off: a number whose
/// country is unknown is stored as it was written (#1676). `None` when
/// fewer than 4 digits remain.
fn digits_as_written(raw: &str) -> Option<String> {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    (digits.len() >= MIN_PHONE_DIGITS).then_some(digits)
}

/// A handle's key, and why a phone number's key is not its `+` form when it
/// is not. [`key_typed_handle`] gives it.
///
/// A phone key carries its country inside it: a key that starts with `+`
/// names its calling code, and one without is a number whose country nobody
/// stated (#1676).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedKey {
    /// The key the handle is stored and matched under.
    pub key: String,
    /// Why a phone number's key is not its certain `+` form, when it is not.
    pub note: Option<String>,
}

/// One normalization policy for a typed handle, shared by the server, the
/// contacts book, and the exporters.
///
/// Phone: guarded E.164 via [`normalize_guarded`] with [`PhoneRegion::for_raw`]:
/// a `+`-prefixed value keeps its own country, a value without `+` is read
/// in the national form of `country`, and with no country it keeps its
/// digits as written. A value with no usable digits falls back to the
/// trimmed raw value, unflagged. Email: lowercased. Username/Other: verbatim
/// (trimmed).
#[must_use]
pub fn key_typed_handle(
    raw: &str,
    handle_type: IdentityType,
    country: Option<&'static Country>,
) -> TypedKey {
    match handle_type {
        IdentityType::Phone => {
            let guarded = normalize_guarded(raw, PhoneRegion::for_raw(raw, country));
            if guarded.normalized.is_empty() {
                TypedKey {
                    key: raw.trim().to_string(),
                    note: None,
                }
            } else {
                TypedKey {
                    key: guarded.normalized,
                    note: guarded.note,
                }
            }
        }
        IdentityType::Email => TypedKey {
            key: raw.trim().to_lowercase(),
            note: None,
        },
        IdentityType::Username | IdentityType::Other => TypedKey {
            key: raw.trim().to_string(),
            note: None,
        },
    }
}

/// [`key_typed_handle`] with no country stated: the key and the note.
#[must_use]
pub fn normalize_typed_handle(raw: &str, handle_type: IdentityType) -> (String, Option<String>) {
    let typed = key_typed_handle(raw, handle_type, None);
    (typed.key, typed.note)
}

/// An address as a source wrote it, classified once: what kind of address
/// it is and the key it is stored and matched under.
///
/// [`Handle::parse`] is the one place an address is classified, from its raw
/// value with its `+` intact. The raw value travels with the handle, because
/// [`OwnerHandleSet::is_owner`] needs to know whether the source wrote a
/// number with its country or in national form.
#[derive(Debug, Clone)]
pub struct Handle {
    kind: IdentityType,
    key: String,
    raw: String,
}

impl Handle {
    /// Classify an address:
    ///
    /// - one with `@` is an email address, keyed in lower case;
    /// - one written as a phone number (digits and phone punctuation, after
    ///   an optional `tel:`) is a phone number, keyed by
    ///   [`normalize_typed_handle`]: a `+` before the first digit keeps the
    ///   country, and a number without one keeps its digits as written,
    ///   because nothing at this point says which country it is in;
    /// - anything else is a sender name such as `AMAZON`, an identity of type
    ///   `other` keyed by the name as written.
    ///
    /// `None` when the value is blank.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Self::parse_in(raw, None)
    }

    /// [`Handle::parse`] for a source whose phone's country is stated: a
    /// number written without its `+` code is keyed in its `+` form in
    /// `country` (#1676). With `None`, it keeps its digits as written.
    #[must_use]
    pub fn parse_in(raw: &str, country: Option<&'static Country>) -> Option<Self> {
        let value = raw.trim();
        if value.is_empty() {
            return None;
        }
        let kind = if value.contains('@') {
            IdentityType::Email
        } else if is_written_as_a_number(value) {
            IdentityType::Phone
        } else {
            IdentityType::Other
        };
        Some(Self::typed_in(value, kind, country))
    }

    /// A handle of a kind the caller already knows.
    #[cfg(test)]
    fn typed(raw: &str, kind: IdentityType) -> Self {
        Self::typed_in(raw, kind, None)
    }

    /// A handle of a kind the caller already knows, keyed in `country`.
    fn typed_in(raw: &str, kind: IdentityType, country: Option<&'static Country>) -> Self {
        Self {
            kind,
            key: key_typed_handle(raw, kind, country).key,
            raw: raw.trim().to_string(),
        }
    }

    /// What kind of address this is.
    #[must_use]
    pub fn kind(&self) -> IdentityType {
        self.kind
    }

    /// The key the address is stored and matched under.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The key, taken out of the handle.
    #[must_use]
    pub fn into_key(self) -> String {
        self.key
    }
}

/// Whether a value is written as a phone number: after an optional `tel:`,
/// at least one digit and nothing but digits and the punctuation a dialled
/// number carries.
fn is_written_as_a_number(value: &str) -> bool {
    let number = value
        .get(..4)
        .filter(|scheme| scheme.eq_ignore_ascii_case("tel:"))
        .map_or(value, |_| &value[4..]);
    number.chars().any(|c| c.is_ascii_digit())
        && number
            .chars()
            .all(|c| c.is_ascii_digit() || PHONE_PUNCTUATION.contains(&c) || c == '#' || c == '*')
}

/// All configured owner handles, each stored under its handle key, in the
/// order they were given.
///
/// The key is [`normalize_typed_handle`], the same function the server uses
/// for its `handles` rows and the contacts book uses for its entries, so an
/// owner phone written `+44 7700 900123` is `+447700900123` here too.
#[derive(Debug, Clone)]
pub struct OwnerHandleSet {
    handles: Vec<(String, IdentityType)>,
}

impl OwnerHandleSet {
    /// Build the set from raw `(value, IdentityType)` pairs; errors when the list
    /// is empty or a phone has no usable digits. The order is kept, and a
    /// handle given twice is kept once.
    ///
    /// # Errors
    ///
    /// Returns an error when `handles` is empty, or when a `IdentityType::Phone`
    /// value sanitizes to no usable digits.
    pub fn new(handles: &[(String, IdentityType)]) -> Result<Self> {
        Self::new_in(handles, None)
    }

    /// [`OwnerHandleSet::new`], reading an owner phone written without its
    /// `+` code in `country`, the country the run states, so it is keyed as
    /// the archive's numbers in that country are.
    ///
    /// # Errors
    ///
    /// As [`OwnerHandleSet::new`].
    pub fn new_in(
        handles: &[(String, IdentityType)],
        country: Option<&'static Country>,
    ) -> Result<Self> {
        if handles.is_empty() {
            bail!("the backup device's phone number or email address is required");
        }
        let mut keyed: Vec<(String, IdentityType)> = Vec::new();
        for (raw, handle_type) in handles {
            if *handle_type == IdentityType::Phone {
                sanitize_number(raw)
                    .with_context(|| format!("owner phone has no usable digits: {raw}"))?;
            }
            let entry = (
                key_typed_handle(raw, *handle_type, country).key,
                *handle_type,
            );
            if !keyed.contains(&entry) {
                keyed.push(entry);
            }
        }
        Ok(Self { handles: keyed })
    }

    /// Whether an address is one of the owner's.
    ///
    /// Every handle matches by handle key. A phone also matches by its
    /// [`sanitize_number`] digits, for sources that record numbers with the
    /// `+` already gone.
    /// Without the `+`, `6555550100` could be Singapore or the US; the
    /// source has already thrown that information away.
    ///
    /// A value written without `+` also matches an owner number given with
    /// `+` when it is that number in national form: the national number
    /// alone, or the trunk prefix `0` followed by it. A carrier often lists
    /// the owner that way in an MMS, so `07700900123` is the owner
    /// `+447700900123`.
    pub fn is_owner(&self, handle: &Handle) -> bool {
        let Handle { kind, key, raw } = handle;
        if *kind != IdentityType::Phone {
            return self.handles.iter().any(|(v, t)| t == kind && v == key);
        }
        let Some(digits) = sanitize_number(raw) else {
            return false;
        };
        let national_form = !raw.contains('+');
        self.phone_keys().any(|owner| {
            owner == key
                || sanitize_number(owner).is_some_and(|d| d == digits)
                || (national_form && is_national_form_of(&digits, owner))
        })
    }

    /// Convenience for exporters that only know about phone numbers.
    ///
    /// # Errors
    ///
    /// As [`OwnerHandleSet::new`].
    pub fn from_phones(phones: &[String]) -> Result<Self> {
        Self::from_phones_in(phones, None)
    }

    /// [`OwnerHandleSet::from_phones`] in the country the run states.
    ///
    /// # Errors
    ///
    /// As [`OwnerHandleSet::new`].
    pub fn from_phones_in(phones: &[String], country: Option<&'static Country>) -> Result<Self> {
        let handles: Vec<(String, IdentityType)> = phones
            .iter()
            .map(|p| (p.clone(), IdentityType::Phone))
            .collect();
        Self::new_in(&handles, country)
    }

    /// The handle key of the first owner phone given, for callers that need
    /// a single owner value (e.g. `owner_identity` in export metadata).
    ///
    /// `None` only when the set holds no phone-typed handles. A set built
    /// with [`OwnerHandleSet::from_phones`] always returns `Some`: that
    /// constructor rejects an empty list and types every entry as a phone.
    pub fn primary_owner_handle(&self) -> Option<String> {
        self.phone_keys().next().map(str::to_string)
    }

    /// The handle keys of the owner's phones, in the order given.
    fn phone_keys(&self) -> impl Iterator<Item = &str> {
        self.handles
            .iter()
            .filter(|(_, t)| *t == IdentityType::Phone)
            .map(|(v, _)| v.as_str())
    }
}

/// Whether `digits` are the national form of the E.164 key `owner`: its
/// national number, alone or after the trunk prefix `0`. An owner key
/// without `+` has no country to strip, so nothing is its national form.
fn is_national_form_of(digits: &str, owner: &str) -> bool {
    let Some(owner_digits) = owner.strip_prefix('+') else {
        return false;
    };
    let (country, national) = split_country_calling_code(owner_digits, true);
    !country.is_empty() && (digits == national || digits.strip_prefix('0') == Some(national))
}

/// A group chat's id and display title from the [`Handle`] keys of its
/// non-owner participants. The id is `prefix` plus a length-prefixed slug of
/// the sorted, deduplicated keys, so `["12","34"]` and `["123","4"]` cannot
/// collide, hashed when it would pass 180 bytes so it stays a safe file
/// name. The title names up to four of the keys.
pub fn group_chat_id(prefix: &str, others: &[String]) -> (String, String) {
    let mut sorted = others.to_vec();
    sorted.sort();
    sorted.dedup();
    let title = if sorted.is_empty() {
        "Group".to_string()
    } else if sorted.len() <= 4 {
        format!("Group: {}", sorted.join(", "))
    } else {
        format!(
            "Group: {}, and {} others",
            sorted[..4].join(", "),
            sorted.len() - 4
        )
    };
    let id = format!("{prefix}{}", group_id_slug(&sorted));
    let id = if id.len() > 180 {
        let digest = hex::encode(Sha256::digest(id.as_bytes()));
        format!("{prefix}{}", &digest[..16])
    } else {
        id
    };
    (id, title)
}

/// Length-prefix each number so `["12","34"]` and `["123","4"]` cannot both
/// become `12_34`.
fn group_id_slug(digits: &[String]) -> String {
    digits
        .iter()
        .map(|d| format!("{}:{}", d.len(), d))
        .collect::<Vec<_>>()
        .join("_")
}

// The note on which test phone numbers are no one's is the comment at the
// top of `tests.rs`. Comments across the workspace point to it.
#[cfg(test)]
mod tests;
