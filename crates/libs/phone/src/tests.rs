//! Test numbers are no one's, in this crate and every other one. Each comes
//! from a range its regulator reserves for fiction. Where a country reserves
//! none, or a test needs a number outside that range, the number starts with
//! a digit no number there starts with, or has a length no number there has.
//!
//! - North America: 555-0100 to 555-0199, in any area code. Source: NANPA,
//!   "Numbering Resources - 555 Line Numbers"
//!   (<https://www.nationalnanpa.com/number_resource_info/555_numbers.html>).
//!   A seven-digit 555-01xx, with or without a leading 1, is no one's
//!   either, since it takes its area code from where it is dialled.
//!   A test that needs a number outside that range puts it under an area
//!   code starting with 0 or 1, as the `demo-seed` test of its fiction check
//!   does with 015. An area code starts with 2 to 9, never 0 or 1. Source:
//!   NANPA, "Number Resources - NPA (Area) Codes"
//!   (<https://www.nationalnanpa.com/area_codes/index.html>).
//! - UK: 020 7946 0xxx and 07700 900xxx. Source: Ofcom, "Telephone numbers
//!   for use in TV and radio drama programmes"
//!   (<https://www.ofcom.org.uk/phones-and-broadband/phone-numbers/numbers-for-drama>).
//!   The `demo-seed` test of its fiction check also uses +44 7700 9001000
//!   and +44 7700 900. A UK mobile number has six digits after 7700, and
//!   these have seven and three. Source: Ofcom, "The National Telephone
//!   Numbering Plan", B2.1: a number has ten digits after the leading 0
//!   (<https://www.ofcom.org.uk/siteassets/resources/documents/phones-telecoms-and-internet/information-for-industry/numbering/other/national-numbering-plan.pdf>).
//! - France: 06 39 98 xx xx. Source: ARCEP decision 2018-0881, Plan
//!   national de numérotation, "Numéros pour œuvres audiovisuelles", page 54
//!   (<https://www.arcep.fr/uploads/tx_gsavis/18-0881.pdf>).
//! - Norway: 68 05 00 00 to 68 05 99 99. Source: Nkom, "Alle nummerserier
//!   for norske telefonnumre", the series for TV and film production
//!   (<https://nkom.no/telefoni-og-telefonnummer/telefonnummer-og-den-norske-nummerplan/alle-nummerserier-for-norske-telefonnumre>).
//! - Denmark reserves no numbers for fiction, as Energistyrelsen told
//!   Søndag Aften (<https://soendagaften.dk/2020/10/sverige-telefonnumre-reserveres-til-kunst/>).
//!   An eight-digit Danish number starts with 2 to 9, so the tests use
//!   +45 0123 4567. Source: BEK nr. 1883 af 07/12/2020, § 11, stk. 2
//!   (<https://www.retsinformation.dk/eli/lta/2020/1883>).
//! - Singapore reserves no numbers for fiction. Leading digit 5 is
//!   "Reserved for future use", so the tests use +65 5555 0100. Source:
//!   IMDA, National Numbering Plan, Table 2.1
//!   (<https://www.imda.gov.sg/-/media/imda/files/regulation-licensing-and-consultations/frameworks-and-policies/numbering/national-numbering-plan-and-allocation-process/imda-national-numbering-plan.pdf>).
//!   Read as a US number without its `+`, it is still in 555-0100.
//! - `+7700900123`, in `an_owner_given_with_plus_matches_its_national_form`,
//!   is the UK drama number 07700 900123 written after `+7` instead of
//!   `+44`. It has nine digits after the 7, one short of any number under
//!   country code 7.

use super::*;

#[test]
fn typed_handle_policy_matches_the_server_for_international_numbers() {
    // The contacts book and the server must key this identically:
    // for_raw keeps the + signal, so the E.164 form survives.
    let (uk, note) = normalize_typed_handle("+44 20 7946 0000", IdentityType::Phone);
    assert_eq!(uk, "+442079460000");
    assert!(note.is_none());
    // Without `+` and with no country stated, the digits as written.
    let (us, note) = normalize_typed_handle("(555) 555-0100", IdentityType::Phone);
    assert_eq!(us, "5555550100");
    assert!(note.is_some());
    let (email, _) = normalize_typed_handle(" Bob@Example.COM ", IdentityType::Email);
    assert_eq!(email, "bob@example.com");
    let (wordy, _) = normalize_typed_handle("no digits here", IdentityType::Phone);
    assert_eq!(wordy, "no digits here");
}

#[test]
fn sanitize_strips_plus_one() {
    assert_eq!(
        sanitize_number("+15555550100").as_deref(),
        Some("5555550100")
    );
    assert_eq!(
        sanitize_number("(555) 555-0101").as_deref(),
        Some("5555550101")
    );
    assert_eq!(sanitize_number(""), None);
    assert_eq!(sanitize_number("4"), None);
    assert_eq!(sanitize_number("06"), None);
}

/// The strict form, which is what a field of unknown content is measured
/// against. See [`sanitize_phone_shaped`] for why the two differ.
#[test]
fn phone_shaped_rejects_prose_and_run_together_numbers() {
    // Written as a number, in every punctuation style a person uses.
    for written in [
        "+1 (555) 555-0119",
        "555.555.0119",
        "(555) 555-0119",
        "  5555550119  ",
    ] {
        assert_eq!(
            sanitize_phone_shaped(written).as_deref(),
            Some("5555550119"),
            "{written} is written as a number"
        );
    }

    // Short codes still pass, as they do for `sanitize_number`.
    assert_eq!(sanitize_phone_shaped("7535").as_deref(), Some("7535"));

    // Prose is rejected before any digits are collected, which is the
    // whole point: `sanitize_number` answers `201942` here.
    assert_eq!(
        sanitize_number("met in 2019 at 42 Acacia Avenue").as_deref(),
        Some("201942")
    );
    assert_eq!(
        sanitize_phone_shaped("met in 2019 at 42 Acacia Avenue"),
        None
    );
    assert_eq!(
        sanitize_phone_shaped("+15555550119 (see also +15555550176)"),
        None
    );
    assert_eq!(
        sanitize_phone_shaped("555-0123 x99"),
        None,
        "an extension is prose"
    );
    assert_eq!(sanitize_phone_shaped("home/cell"), None);

    // Two numbers with nothing but permitted punctuation between them are
    // caught by the digit ceiling instead.
    assert_eq!(sanitize_phone_shaped("+15555550119 +15555550176"), None);
    assert_eq!(
        sanitize_phone_shaped("123456789012345").as_deref(),
        Some("123456789012345"),
        "fifteen digits is the E.164 maximum, and is allowed"
    );
    assert_eq!(
        sanitize_phone_shaped("1234567890123456"),
        None,
        "sixteen is not"
    );

    // Too few digits, and nothing at all.
    assert_eq!(sanitize_phone_shaped("123"), None);
    assert_eq!(sanitize_phone_shaped(""), None);
    assert_eq!(sanitize_phone_shaped("   "), None);
    assert_eq!(sanitize_phone_shaped("()-."), None);
}

#[test]
fn sanitize_keeps_short_codes() {
    assert_eq!(sanitize_number("7535").as_deref(), Some("7535"));
    assert_eq!(sanitize_number("73737").as_deref(), Some("73737"));
}

fn us() -> PhoneRegion {
    PhoneRegion::Country(country("US").unwrap())
}

#[test]
fn certain_usa() {
    assert_eq!(
        normalize_checked("(542).555-0100", us()).ok().as_deref(),
        Some("+15425550100")
    );
    assert_eq!(
        normalize_checked("1-555-555-0121", us()).ok().as_deref(),
        Some("+15555550121")
    );
    assert!(
        normalize_checked("1555-0145", us()).is_err(),
        "too short for USA certainty"
    );
    // A `+` names its own country, whatever country is stated.
    assert_eq!(
        normalize_checked("+442079460750", us()).ok().as_deref(),
        Some("+442079460750")
    );
}

#[test]
fn guarded_certain_usa() {
    let g = normalize_guarded("5555550100", us());
    assert_eq!(g.normalized, "+15555550100");
    assert_eq!(g.note, None);
    let g = normalize_guarded("15555550100", us());
    assert_eq!(g.normalized, "+15555550100");
    assert_eq!(g.note, None);
}

#[test]
fn guarded_certain_plus_prefixed() {
    let g = normalize_guarded("+44 20 7946 0750", PhoneRegion::International);
    assert_eq!(g.normalized, "+442079460750");
    assert_eq!(g.note, None);
}

#[test]
fn guarded_rejects_fabricated_plus_zero() {
    // E.164 country codes never start with 0: `+020…` must not be trusted
    // as certain even when it has a plausible digit count.
    assert!(normalize_checked("+02079460000", PhoneRegion::International).is_err());
    let g = normalize_guarded("+02079460000", PhoneRegion::International);
    assert_eq!(g.normalized, "02079460000");
    assert_eq!(
        g.note.as_deref(),
        Some("international country code cannot start with 0")
    );
}

#[test]
fn guarded_trunk_zero_stays_digits_with_note() {
    // `020 7946 0000` with no country stated: never `+02079460000`.
    let g = normalize_guarded("020 7946 0000", PhoneRegion::Unknown);
    assert_eq!(g.normalized, "02079460000");
    assert_eq!(
        g.note.as_deref(),
        Some("written without its country code, in no known country")
    );
    // In the US, it is not a number at all.
    let g = normalize_guarded("020 7946 0000", us());
    assert_eq!(g.normalized, "02079460000");
    assert_eq!(
        g.note.as_deref(),
        Some("a number in US has 10 digits, or 11 starting with 1")
    );
    // `442079460000` without `+`: ambiguous in both regions.
    let g = normalize_guarded("442079460000", PhoneRegion::International);
    assert_eq!(g.normalized, "442079460000");
    assert_eq!(
        g.note.as_deref(),
        Some("international mode requires a leading +")
    );
}

#[test]
fn guarded_region_for_raw() {
    let gb = country("GB");
    assert_eq!(
        PhoneRegion::for_raw("+15555550100", gb),
        PhoneRegion::International
    );
    assert_eq!(
        PhoneRegion::for_raw("  +44 20 7946 0750 ", None),
        PhoneRegion::International
    );
    // A number without `+` is not read as a US number (#1676).
    assert_eq!(
        PhoneRegion::for_raw("5555550100", None),
        PhoneRegion::Unknown
    );
    assert_eq!(PhoneRegion::for_raw("", None), PhoneRegion::Unknown);
    assert_eq!(
        PhoneRegion::for_raw("020 7946 0000", gb),
        PhoneRegion::Country(gb.unwrap())
    );
}

#[test]
fn certain_international() {
    assert_eq!(
        normalize_checked("+44 20 7946 0750", PhoneRegion::International)
            .ok()
            .as_deref(),
        Some("+442079460750")
    );
    assert!(
        normalize_checked("(542).555-0100", PhoneRegion::International).is_err(),
        "no leading +"
    );
    assert_eq!(
        normalize_checked("+1-542-555-0100", PhoneRegion::International)
            .ok()
            .as_deref(),
        Some("+15425550100")
    );
}

#[test]
fn checked_rejects_with_fixed_reasons() {
    assert_eq!(
        normalize_checked("+12", PhoneRegion::International),
        Err("international needs 8–15 digits after +".into())
    );
    assert_eq!(
        normalize_checked("442079460000", PhoneRegion::International),
        Err("international mode requires a leading +".into())
    );
    assert_eq!(
        normalize_checked("", PhoneRegion::Unknown),
        Err("empty phone".into())
    );
    assert_eq!(
        normalize_checked("abc", PhoneRegion::Unknown),
        Err("no digits".into())
    );
}

#[test]
fn primary_owner_handle_present_for_phone_sets() {
    let owners = OwnerHandleSet::from_phones(&["(555) 555-0100".into()]).unwrap();
    assert_eq!(owners.primary_owner_handle().as_deref(), Some("5555550100"));
    let trunk = OwnerHandleSet::from_phones(&["020 7946 0000".into()]).unwrap();
    assert_eq!(trunk.primary_owner_handle().as_deref(), Some("02079460000"));
    // Only a phone-free set has no primary owner handle.
    let email_only = OwnerHandleSet::new(&[("a@example.com".into(), IdentityType::Email)]).unwrap();
    assert_eq!(email_only.primary_owner_handle(), None);
}

#[test]
fn owner_set_rejects_empty() {
    // The exporters have no command line, so the message names no flag.
    assert_eq!(
        OwnerHandleSet::new(&[]).unwrap_err().to_string(),
        "the backup device's phone number or email address is required"
    );
    assert!(OwnerHandleSet::from_phones(&[]).is_err());
    assert!(OwnerHandleSet::from_phones(&["not-a-phone".into()]).is_err());
}

#[test]
fn owner_handle_set_matches_typed_handles() {
    let owners = OwnerHandleSet::new(&[
        ("(555) 555-0100".into(), IdentityType::Phone),
        ("Person@Example.COM".into(), IdentityType::Email),
    ])
    .unwrap();
    assert!(owners.is_owner(&Handle::typed("+15555550100", IdentityType::Phone)));
    assert!(owners.is_owner(&Handle::typed("5555550100", IdentityType::Phone)));
    assert!(!owners.is_owner(&Handle::typed("5555550199", IdentityType::Phone)));
    assert!(owners.is_owner(&Handle::typed("person@example.com", IdentityType::Email)));
    assert!(!owners.is_owner(&Handle::typed("other@example.com", IdentityType::Email)));
    // A phone-shaped handle is not treated as an email or username handle.
    assert!(!owners.is_owner(&Handle::typed("(555) 555-0100", IdentityType::Email)));
    assert!(!owners.is_owner(&Handle::typed("Person@Example.COM", IdentityType::Username)));
}

#[test]
fn the_first_owner_phone_given_is_the_primary_owner_handle() {
    // Nine numbers: a set's order would pick the first given only by chance.
    let phones: Vec<String> = [9, 0, 1, 2, 3, 4, 5, 6, 7]
        .iter()
        .map(|i| format!("+1555555010{i}"))
        .collect();
    let owners = OwnerHandleSet::from_phones(&phones).unwrap();
    assert_eq!(
        owners.primary_owner_handle().as_deref(),
        Some("+15555550109")
    );
}

#[test]
fn an_owner_given_with_plus_matches_its_national_form() {
    let owners = OwnerHandleSet::from_phones(&["+447700900123".into()]).unwrap();
    assert!(owners.is_owner(&Handle::typed("07700900123", IdentityType::Phone)));
    assert!(owners.is_owner(&Handle::typed("07700 900123", IdentityType::Phone)));
    assert!(owners.is_owner(&Handle::typed("7700900123", IdentityType::Phone)));
    assert!(!owners.is_owner(&Handle::typed("07700900124", IdentityType::Phone)));
    assert!(!owners.is_owner(&Handle::typed("0447700900123", IdentityType::Phone)));
    // A value with `+` is in international form and names its country.
    assert!(!owners.is_owner(&Handle::typed("+7700900123", IdentityType::Phone)));
    // An owner given without `+` has no country to strip.
    let local = OwnerHandleSet::from_phones(&["020 7946 0000".into()]).unwrap();
    assert!(!local.is_owner(&Handle::typed("2079460000", IdentityType::Phone)));
}

#[test]
fn an_international_owner_keeps_its_country() {
    let owners = OwnerHandleSet::from_phones(&["+44 7700 900123".into()]).unwrap();
    assert_eq!(
        owners.primary_owner_handle().as_deref(),
        Some("+447700900123")
    );
    assert!(owners.is_owner(&Handle::typed("+447700900123", IdentityType::Phone)));
    // A source that has already dropped the `+` matches by digits.
    assert!(owners.is_owner(&Handle::typed("447700900123", IdentityType::Phone)));
    assert!(!owners.is_owner(&Handle::typed("447700900999", IdentityType::Phone)));
    assert!(!owners.is_owner(&Handle::typed("06", IdentityType::Phone)));
}

#[test]
fn an_owner_given_in_national_form_matches_its_plus_form_in_the_run_s_country() {
    let national = ["07700900123".to_string()];
    let full = Handle::parse("+447700900123").unwrap();
    let owners = OwnerHandleSet::from_phones_in(&national, country("GB")).unwrap();
    assert!(owners.is_owner(&full));
    assert_eq!(
        owners.primary_owner_handle().as_deref(),
        Some("+447700900123")
    );
    // With no country stated, the national form names no country.
    assert!(
        !OwnerHandleSet::from_phones(&national)
            .unwrap()
            .is_owner(&full)
    );
}

#[test]
fn owner_handle_set_guards_trunk_zero() {
    let owners =
        OwnerHandleSet::new(&[("020 7946 0000".to_string(), IdentityType::Phone)]).unwrap();
    assert!(owners.is_owner(&Handle::typed("02079460000", IdentityType::Phone)));
    assert!(owners.is_owner(&Handle::typed("020 7946 0000", IdentityType::Phone)));
    // The digits-as-is identity is never fabricated into +02079460000, so
    // a +0… message handle matches through the same digit stripping.
    assert!(owners.is_owner(&Handle::typed("+02079460000", IdentityType::Phone)));
    assert!(!owners.is_owner(&Handle::typed("+02079469999", IdentityType::Phone)));
}

#[test]
fn a_plus_before_the_first_digit_keeps_the_country() {
    for raw in ["(+44) 7700 900123", "tel:+447700900123"] {
        assert_eq!(
            normalize_typed_handle(raw, IdentityType::Phone).0,
            "+447700900123",
            "{raw}"
        );
    }
}

fn keyed_in(raw: &str, code: &str) -> TypedKey {
    key_typed_handle(raw, IdentityType::Phone, country(code))
}

#[test]
fn a_national_number_in_a_stated_country_takes_its_plus_form() {
    // The trunk prefix goes, and the calling code goes in front.
    let uk = keyed_in("07700 900123", "GB");
    assert_eq!((uk.key.as_str(), uk.note), ("+447700900123", None));
    // Written without the trunk prefix, it is the same number.
    assert_eq!(keyed_in("7700900123", "GB").key, "+447700900123");
    assert_eq!(keyed_in("(555) 555-0100", "US").key, "+15555550100");
    assert_eq!(keyed_in("1 555 555 0100", "CA").key, "+15555550100");
    // Russia dials 8 before a national number, Hungary 06.
    assert_eq!(keyed_in("8 912 345 6789", "RU").key, "+79123456789");
    assert_eq!(keyed_in("06 30 123 4567", "HU").key, "+36301234567");
    // In Italy the leading 0 belongs to the number.
    assert_eq!(keyed_in("06 1234 5678", "IT").key, "+390612345678");
}

#[test]
fn a_country_s_own_international_prefix_starts_a_calling_code() {
    // Each country's own prefix, from libphonenumber: the digits after it
    // are a calling code and number, never a national number (#1676).
    assert_eq!(keyed_in("0044 7700 900123", "GB").key, "+447700900123");
    assert_eq!(keyed_in("011 44 7700 900123", "US").key, "+447700900123");
    assert_eq!(keyed_in("0011 44 7700 900123", "AU").key, "+447700900123");
    assert_eq!(keyed_in("002 44 7700 900123", "KR").key, "+447700900123");
    assert_eq!(keyed_in("8 10 44 7700 900123", "RU").key, "+447700900123");
    assert_eq!(keyed_in("010 65 6123 4567", "JP").key, "+6561234567");
    // Australia's `0011` is not its trunk prefix and a `11`.
    assert!(
        !keyed_in("0011 44 7700 900123", "AU")
            .key
            .starts_with("+611")
    );
}

#[test]
fn digits_after_an_international_prefix_that_name_no_calling_code_are_refused() {
    let refused = keyed_in("00 0123 456789", "GB");
    assert_eq!(refused.key, "000123456789");
    assert!(
        refused
            .note
            .is_some_and(|n| n.contains("not a number GB can read"))
    );
}

#[test]
fn a_number_written_with_its_calling_code_but_no_plus_keeps_it_once() {
    // The rest is as long as the country's numbers are, and the whole is
    // not.
    assert_eq!(keyed_in("447700900123", "GB").key, "+447700900123");
    assert_eq!(keyed_in("79161234567", "RU").key, "+79161234567");
    assert_eq!(keyed_in("33612345678", "FR").key, "+33612345678");
    // Brazil's area code 55 is not its calling code: the whole is as
    // long as a Brazilian mobile number.
    assert_eq!(keyed_in("55 99123 4567", "BR").key, "+5555991234567");
    // An Indian mobile number that starts with 91 is a national number.
    assert_eq!(keyed_in("9112345678", "IN").key, "+919112345678");
}

#[test]
fn a_number_that_reads_as_well_either_way_is_refused() {
    // Germany, Austria and Italy have numbers of both lengths, so the
    // digits could be national or written with the calling code.
    for (raw, code, national, full) in [
        (
            "49 151 23456789",
            "DE",
            "+494915123456789",
            "+4915123456789",
        ),
        ("43 664 1234567", "AT", "+43436641234567", "+436641234567"),
        ("39 333 1234567", "IT", "+39393331234567", "+393331234567"),
    ] {
        let refused = keyed_in(raw, code);
        let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
        assert_eq!(refused.key, digits, "{raw} in {code}");
        assert_eq!(
            refused.note.as_deref(),
            Some(
                format!(
                    "{digits} could be {national} or {full}; \
                     pick its country or write it with +"
                )
                .as_str()
            ),
            "{raw} in {code}"
        );
    }
}

#[test]
fn a_trunk_prefix_kept_after_the_calling_code_is_dropped() {
    assert_eq!(keyed_in("44 (0)7700 900123", "GB").key, "+447700900123");
    assert_eq!(keyed_in("49 (0)151 23456789", "DE").key, "+4915123456789");
}

#[test]
fn a_stated_country_never_overrides_a_plus_or_invents_a_short_code() {
    assert_eq!(keyed_in("+6555550100", "GB").key, "+6555550100");
    let short = keyed_in("7535", "GB");
    assert_eq!(short.key, "7535");
    assert_eq!(
        short.note.as_deref(),
        Some("too few digits for a number in GB")
    );
    // An email address has no country.
    let email = key_typed_handle("Jo@Example.com", IdentityType::Email, country("GB"));
    assert_eq!(email.key, "jo@example.com");
}

#[test]
fn a_handle_parsed_in_a_country_matches_the_plus_form() {
    let national = Handle::parse("07700 900123").unwrap();
    let full = Handle::parse("+447700900123").unwrap();
    assert_ne!(national.key(), full.key(), "no country stated: two keys");
    let gb = country("GB");
    assert_eq!(
        Handle::parse_in("07700 900123", gb).unwrap().key(),
        full.key()
    );
    assert_eq!(
        Handle::parse_in("+447700900123", gb).unwrap().key(),
        full.key()
    );
    // The country never touches an email address or a sender name.
    assert_eq!(Handle::parse_in("AMAZON", gb).unwrap().key(), "AMAZON");
}

fn parsed(raw: &str) -> (IdentityType, String) {
    let handle = Handle::parse(raw).unwrap();
    (handle.kind(), handle.key().to_string())
}

#[test]
fn a_number_written_with_plus_keeps_its_country() {
    // +65 5555 0100 is no one's number: the note on `mod tests` says why.
    assert_eq!(
        parsed("+6555550100"),
        (IdentityType::Phone, "+6555550100".into())
    );
    assert_eq!(
        parsed("+447700900123"),
        (IdentityType::Phone, "+447700900123".into())
    );
    assert_eq!(
        parsed("tel:+447700900123"),
        (IdentityType::Phone, "+447700900123".into())
    );
}

#[test]
fn a_number_without_plus_keeps_its_digits_as_written() {
    // No country is stated, so none is assumed: not the US either (#1676).
    assert_eq!(
        parsed("(555) 555-0100"),
        (IdentityType::Phone, "5555550100".into())
    );
    assert_eq!(
        parsed("15555550100"),
        (IdentityType::Phone, "15555550100".into())
    );
    assert_eq!(
        parsed("020 7946 0000"),
        (IdentityType::Phone, "02079460000".into())
    );
    assert_eq!(parsed("7535"), (IdentityType::Phone, "7535".into()));
}

#[test]
fn an_address_with_at_is_an_email_address_not_the_digits_inside_it() {
    assert_eq!(
        parsed(" John1985@Example.com "),
        (IdentityType::Email, "john1985@example.com".into())
    );
}

#[test]
fn an_address_that_is_neither_a_number_nor_an_email_is_a_sender_name() {
    assert_eq!(parsed("AMAZON"), (IdentityType::Other, "AMAZON".into()));
    assert_eq!(parsed(" Bank 24 "), (IdentityType::Other, "Bank 24".into()));
    assert!(Handle::parse("   ").is_none());
}

#[test]
fn a_handle_key_parses_to_itself() {
    for raw in [
        "+6555550100",
        "5555550100",
        "7535",
        "jo@example.com",
        "AMAZON",
    ] {
        let (kind, key) = parsed(raw);
        assert_eq!(parsed(&key), (kind, key.clone()), "{raw}");
    }
}

#[test]
fn the_owner_test_takes_a_handle() {
    let owners = OwnerHandleSet::from_phones(&["+447700900123".into()]).unwrap();
    assert!(owners.is_owner(&Handle::parse("07700 900123").unwrap()));
    assert!(owners.is_owner(&Handle::parse("7700900123").unwrap()));
    assert!(!owners.is_owner(&Handle::parse("AMAZON").unwrap()));
}

fn keys(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("+155555501{i:02}")).collect()
}

fn numbers(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("55555501{i:02}")).collect()
}

#[test]
fn group_chat_id_names_up_to_four_numbers() {
    let others = vec![
        "+15555550102".to_string(),
        "+15555550101".to_string(),
        "+15555550102".to_string(),
    ];
    assert_eq!(
        group_chat_id("grp-", &others),
        (
            "grp-12:+15555550101_12:+15555550102".to_string(),
            "Group: +15555550101, +15555550102".to_string()
        ),
        "sorted, deduplicated, length-prefixed id; the keys in the title"
    );
    assert_eq!(group_chat_id("grp-", &[]).1, "Group");
    assert_eq!(
        group_chat_id("grp-", &keys(4)).1,
        "Group: +15555550100, +15555550101, +15555550102, +15555550103"
    );
}

#[test]
fn a_group_title_keeps_each_number_s_country() {
    let others = ["+6555550100".to_string(), "+447700900123".to_string()];
    assert_eq!(
        group_chat_id("grp-", &others).1,
        "Group: +447700900123, +6555550100"
    );
}

#[test]
fn group_chat_id_counts_the_numbers_past_four() {
    assert_eq!(
        group_chat_id("grp-", &keys(5)).1,
        "Group: +15555550100, +15555550101, +15555550102, +15555550103, and 1 others"
    );
}

#[test]
fn group_chat_id_hashes_an_id_past_180_bytes() {
    // 20 numbers make a 283-byte id. The expected value is the first 16
    // hex digits of SHA-256 over that id, computed with Python's hashlib.
    let mut others = numbers(20);
    assert_eq!(group_chat_id("grp-", &others).0, "grp-03ecb51fee88d135");
    others.reverse();
    assert_eq!(
        group_chat_id("grp-", &others).0,
        "grp-03ecb51fee88d135",
        "the order the numbers arrive in doesn't change the id"
    );

    // 12 numbers behind a 13-byte prefix make exactly 180 bytes: kept.
    let (id, _) = group_chat_id("sms-backup-g-", &numbers(12));
    assert_eq!(id.len(), 180);
    assert!(id.starts_with("sms-backup-g-10:5555550100_"), "{id}");
}
