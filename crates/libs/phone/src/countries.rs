//! The countries a person can name for a phone number written without its
//! `+` code, with what the number needs to be written in full.
//!
//! A country here is a fact a person or a source states, never a guess: the
//! import form's country for a run, or the country picked for one identity
//! (`docs/architecture/contacts-identities-and-messages.md`, "A phone number
//! carries its country"). Nothing in this crate picks one from the digits.

/// One country a phone number can belong to.
#[derive(Debug, PartialEq, Eq)]
pub struct Country {
    /// ISO 3166-1 alpha-2 code, upper case: `GB`, `US`. `XK` is Kosovo, which
    /// has a calling code and no ISO code of its own.
    pub code: &'static str,
    /// Country calling code without the `+`: `44`, `1`. Several countries
    /// share one (`1` is the United States, Canada and much of the
    /// Caribbean), because they share one numbering plan.
    pub calling_code: &'static str,
    /// The trunk prefix a national number is dialled with inside the country
    /// and dropped from its `+` form: `0` in most countries, `8` in Russia,
    /// `06` in Hungary, empty where the leading digit belongs to the number
    /// (Italy's `06` is part of a Rome number). The calling code `1` is
    /// written by its own rule and does not read this.
    pub trunk_prefix: &'static str,
    /// The international prefix dialled from the country before a calling
    /// code, as libphonenumber's regular expression: `00` in most countries,
    /// `011` in a `+1` country, `810` in Russia, `010` in Japan, and several
    /// forms in some (`001[14-689]|…` in Australia).
    pub international_prefix: &'static str,
    /// How many digits a fixed-line or mobile number of the country has
    /// without its trunk prefix and calling code.
    pub national_lengths: &'static [u8],
}

/// Shorthand for a table row.
const fn c(
    code: &'static str,
    calling_code: &'static str,
    trunk_prefix: &'static str,
    international_prefix: &'static str,
    national_lengths: &'static [u8],
) -> Country {
    Country {
        code,
        calling_code,
        trunk_prefix,
        international_prefix,
        national_lengths,
    }
}

/// Every country, by ISO code.
///
/// The calling codes, trunk prefixes and international prefixes are the
/// `countryCode`, `nationalPrefix` and `internationalPrefix` attributes of
/// each `<territory>` in libphonenumber's
/// `resources/PhoneNumberMetadata.xml`, at commit `d1457634c3d9`
/// (2026-10-07). The national lengths are the `national` attribute of the
/// `<possibleLengths>` of its `<fixedLine>` and its `<mobile>`, together:
/// <https://github.com/google/libphonenumber/blob/d1457634c3d95e570dad433ee0ab400beb343a56/resources/PhoneNumberMetadata.xml>.
/// The table departs from it in two ways. The calling code `1` rows carry no
/// trunk prefix, because [`crate::normalize_checked`] writes a `+1` number by
/// its own rule; libphonenumber gives them `1`. `AC` (Ascension) and `TA`
/// (Tristan da Cunha) are left out, because ISO 3166-1 gives them no code of
/// their own. A trunk prefix is checked against that file before it is
/// changed, because a wrong one gives a number a `+` form that names nobody.
pub const COUNTRIES: &[Country] = &[
    c("AD", "376", "", r"00", &[6, 9]),
    c("AE", "971", "0", r"00", &[8, 9]),
    c("AF", "93", "0", r"00", &[9]),
    c("AG", "1", "", r"011", &[10]),
    c("AI", "1", "", r"011", &[10]),
    c("AL", "355", "0", r"00", &[8, 9]),
    c("AM", "374", "0", r"00", &[8]),
    c("AO", "244", "", r"00", &[9]),
    c("AR", "54", "0", r"00", &[10, 11]),
    c("AS", "1", "", r"011", &[10]),
    c("AT", "43", "0", r"00", &[4, 5, 6, 7, 8, 9, 10, 11, 12, 13]),
    c(
        "AU",
        "61",
        "0",
        r"001[14-689]|14(?:1[14]|34|4[17]|[56]6|7[47]|88)0011",
        &[9],
    ),
    c("AW", "297", "", r"00", &[7]),
    c(
        "AX",
        "358",
        "0",
        r"00|99(?:[01469]|5(?:[14]1|3[23]|5[59]|77|88|9[09]))",
        &[6, 7, 8, 9, 10],
    ),
    c("AZ", "994", "0", r"00", &[9]),
    c("BA", "387", "0", r"00", &[8, 9]),
    c("BB", "1", "", r"011", &[10]),
    c("BD", "880", "0", r"00", &[6, 7, 8, 9, 10]),
    c("BE", "32", "0", r"00", &[8, 9]),
    c("BF", "226", "", r"00", &[8]),
    c("BG", "359", "0", r"00", &[6, 7, 8, 9]),
    c("BH", "973", "", r"00", &[8]),
    c("BI", "257", "", r"00", &[8]),
    c("BJ", "229", "", r"00", &[10]),
    c("BL", "590", "0", r"00", &[9]),
    c("BM", "1", "", r"011", &[10]),
    c("BN", "673", "", r"00", &[7]),
    c("BO", "591", "0", r"00(?:1\d)?", &[8]),
    c("BQ", "599", "", r"00", &[7]),
    c(
        "BR",
        "55",
        "0",
        r"00(?:1[245]|2[1-35]|31|4[13]|[56]5|99)",
        &[10, 11],
    ),
    c("BS", "1", "", r"011", &[10]),
    c("BT", "975", "", r"00", &[7, 8]),
    c("BW", "267", "", r"00", &[7, 8]),
    c("BY", "375", "8", r"810", &[9]),
    c("BZ", "501", "", r"00", &[7]),
    c("CA", "1", "", r"011", &[10]),
    c(
        "CC",
        "61",
        "0",
        r"001[14-689]|14(?:1[14]|34|4[17]|[56]6|7[47]|88)0011",
        &[9],
    ),
    c("CD", "243", "0", r"00", &[7, 8, 9, 10]),
    c("CF", "236", "", r"00", &[8]),
    c("CG", "242", "", r"00", &[9]),
    c("CH", "41", "0", r"00", &[9]),
    c("CI", "225", "", r"00", &[10]),
    c("CK", "682", "", r"00", &[5]),
    c(
        "CL",
        "56",
        "",
        r"(?:0|1(?:1[0-69]|2[02-5]|5[13-58]|69|7[0167]|8[018]))0",
        &[9],
    ),
    c("CM", "237", "", r"00", &[9]),
    c(
        "CN",
        "86",
        "0",
        r"00|1(?:[12]\d|79)\d\d00",
        &[7, 8, 9, 10, 11],
    ),
    c("CO", "57", "0", r"00(?:4(?:[14]4|56)|[579])", &[8, 10]),
    c("CR", "506", "", r"00", &[8]),
    c("CU", "53", "0", r"119", &[6, 7, 8, 10]),
    c("CV", "238", "", r"0", &[7]),
    c("CW", "599", "", r"00", &[7, 8]),
    c(
        "CX",
        "61",
        "0",
        r"001[14-689]|14(?:1[14]|34|4[17]|[56]6|7[47]|88)0011",
        &[9],
    ),
    c("CY", "357", "", r"00", &[8]),
    c("CZ", "420", "", r"00", &[9]),
    c(
        "DE",
        "49",
        "0",
        r"00",
        &[5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    ),
    c("DJ", "253", "", r"00", &[8]),
    c("DK", "45", "", r"00", &[8]),
    c("DM", "1", "", r"011", &[10]),
    c("DO", "1", "", r"011", &[10]),
    c("DZ", "213", "0", r"00", &[8, 9]),
    c("EC", "593", "0", r"00", &[8, 9]),
    c("EE", "372", "", r"00", &[7, 8]),
    c("EG", "20", "0", r"00", &[8, 9, 10]),
    c("EH", "212", "0", r"00", &[9]),
    c("ER", "291", "0", r"00", &[7]),
    c("ES", "34", "", r"00", &[9]),
    c("ET", "251", "0", r"00", &[9]),
    c(
        "FI",
        "358",
        "0",
        r"00|99(?:[01469]|5(?:[14]1|3[23]|5[59]|77|88|9[09]))",
        &[5, 6, 7, 8, 9, 10],
    ),
    c("FJ", "679", "", r"0(?:0|52)", &[7]),
    c("FK", "500", "", r"00", &[5]),
    c("FM", "691", "", r"00", &[7]),
    c("FO", "298", "", r"00", &[6]),
    c("FR", "33", "0", r"00", &[9]),
    c("GA", "241", "", r"00", &[7, 8]),
    c("GB", "44", "0", r"00", &[9, 10]),
    c("GD", "1", "", r"011", &[10]),
    c("GE", "995", "0", r"00", &[9]),
    c("GF", "594", "0", r"00", &[9]),
    c("GG", "44", "0", r"00", &[10]),
    c("GH", "233", "0", r"00", &[9]),
    c("GI", "350", "", r"00", &[8]),
    c("GL", "299", "", r"00", &[6]),
    c("GM", "220", "", r"00", &[7, 9]),
    c("GN", "224", "", r"00", &[8, 9]),
    c("GP", "590", "0", r"00", &[9]),
    c("GQ", "240", "", r"00", &[9]),
    c("GR", "30", "", r"00", &[10]),
    c("GT", "502", "", r"00", &[8]),
    c("GU", "1", "", r"011", &[10]),
    c("GW", "245", "", r"00", &[9]),
    c("GY", "592", "", r"001", &[7]),
    c("HK", "852", "", r"00(?:30|5[09]|[126-9]?)", &[8]),
    c("HN", "504", "", r"00", &[8]),
    c("HR", "385", "0", r"00", &[8, 9]),
    c("HT", "509", "", r"00", &[8]),
    c("HU", "36", "06", r"00", &[8, 9]),
    c("ID", "62", "0", r"00[89]", &[7, 8, 9, 10, 11, 12]),
    c("IE", "353", "0", r"00", &[7, 8, 9, 10]),
    c("IL", "972", "0", r"0(?:0|1(?:05|[2-9]))", &[8, 9, 11, 12]),
    c("IM", "44", "0", r"00", &[10]),
    c("IN", "91", "0", r"00", &[10]),
    c("IO", "246", "", r"00", &[7]),
    c("IQ", "964", "0", r"00", &[8, 9, 10]),
    c("IR", "98", "0", r"00", &[6, 7, 10]),
    c("IS", "354", "", r"00|1(?:0(?:01|[12]0)|100)", &[7, 9]),
    c("IT", "39", "", r"00", &[6, 7, 8, 9, 10, 11, 12]),
    c("JE", "44", "0", r"00", &[10]),
    c("JM", "1", "", r"011", &[10]),
    c("JO", "962", "0", r"00", &[8, 9]),
    c("JP", "81", "0", r"010", &[9, 10]),
    c("KE", "254", "0", r"000", &[7, 8, 9]),
    c("KG", "996", "0", r"00", &[9]),
    c("KH", "855", "0", r"00[14-9]", &[8, 9]),
    c("KI", "686", "0", r"00", &[5, 8]),
    c("KM", "269", "", r"00", &[7]),
    c("KN", "1", "", r"011", &[10]),
    c("KP", "850", "0", r"00|99", &[8, 10]),
    c(
        "KR",
        "82",
        "0",
        r"00(?:[125689]|3(?:[46]5|91)|7(?:00|27|3|55|6[126]))",
        &[5, 6, 8, 9, 10],
    ),
    c("KW", "965", "", r"00", &[8]),
    c("KY", "1", "", r"011", &[10]),
    c("KZ", "7", "8", r"810", &[10]),
    c("LA", "856", "0", r"00", &[8, 9, 10]),
    c("LB", "961", "0", r"00", &[7, 8]),
    c("LC", "1", "", r"011", &[10]),
    c("LI", "423", "0", r"00", &[7, 9]),
    c("LK", "94", "0", r"00", &[9]),
    c("LR", "231", "0", r"00", &[7, 8, 9]),
    c("LS", "266", "", r"00", &[8]),
    c("LT", "370", "0", r"00", &[8]),
    c("LU", "352", "", r"00", &[4, 5, 6, 7, 8, 9, 10, 11]),
    c("LV", "371", "", r"00", &[8]),
    c("LY", "218", "0", r"00", &[9]),
    c("MA", "212", "0", r"00", &[9]),
    c("MC", "377", "0", r"00", &[8, 9]),
    c("MD", "373", "0", r"00", &[8]),
    c("ME", "382", "0", r"00", &[8]),
    c("MF", "590", "0", r"00", &[9]),
    c("MG", "261", "0", r"00", &[9]),
    c("MH", "692", "1", r"011", &[7]),
    c("MK", "389", "0", r"00", &[8]),
    c("ML", "223", "", r"00", &[8]),
    c("MM", "95", "0", r"00", &[6, 7, 8, 9, 10]),
    c("MN", "976", "0", r"001", &[8, 9, 10]),
    c("MO", "853", "", r"00", &[8]),
    c("MP", "1", "", r"011", &[10]),
    c("MQ", "596", "0", r"00", &[9]),
    c("MR", "222", "", r"00", &[8]),
    c("MS", "1", "", r"011", &[10]),
    c("MT", "356", "", r"00", &[8]),
    c("MU", "230", "", r"0(?:0|[24-7]0|3[03])", &[7, 8]),
    c("MV", "960", "", r"0(?:0|19)", &[7]),
    c("MW", "265", "0", r"00", &[7, 9]),
    c("MX", "52", "", r"0[09]", &[10]),
    c("MY", "60", "0", r"00", &[8, 9, 10]),
    c("MZ", "258", "", r"00", &[8, 9]),
    c("NA", "264", "0", r"00", &[8, 9]),
    c("NC", "687", "", r"00", &[6]),
    c("NE", "227", "", r"00", &[8]),
    c("NF", "672", "", r"00", &[6]),
    c("NG", "234", "0", r"009", &[10]),
    c("NI", "505", "", r"00", &[8]),
    c("NL", "31", "0", r"00", &[9, 11]),
    c("NO", "47", "", r"00", &[8]),
    c("NP", "977", "0", r"00", &[8, 10]),
    c("NR", "674", "", r"00", &[7]),
    c("NU", "683", "", r"00", &[4, 7]),
    c("NZ", "64", "0", r"0(?:0|161)", &[8, 9, 10]),
    c("OM", "968", "", r"00", &[8]),
    c("PA", "507", "", r"00", &[7, 8]),
    c("PE", "51", "0", r"00|19(?:1[124]|77|90)00", &[8, 9]),
    c("PF", "689", "", r"00", &[8]),
    c("PG", "675", "", r"00|140[1-3]", &[7, 8]),
    c("PH", "63", "0", r"00", &[6, 8, 9, 10]),
    c("PK", "92", "0", r"00", &[9, 10]),
    c("PL", "48", "", r"00", &[7, 9]),
    c("PM", "508", "0", r"00", &[6, 9]),
    c("PR", "1", "", r"011", &[10]),
    c("PS", "970", "0", r"00", &[8, 9]),
    c("PT", "351", "", r"00", &[9]),
    c("PW", "680", "", r"01[12]", &[7]),
    c("PY", "595", "0", r"00", &[7, 8, 9]),
    c("QA", "974", "", r"00", &[8]),
    c("RE", "262", "0", r"00", &[9]),
    c("RO", "40", "0", r"00", &[6, 9]),
    c("RS", "381", "0", r"00", &[7, 8, 9, 10, 11, 12]),
    c("RU", "7", "8", r"810", &[10]),
    c("RW", "250", "0", r"00", &[8, 9]),
    c("SA", "966", "0", r"00", &[9]),
    c("SB", "677", "", r"0[01]", &[5, 7]),
    c("SC", "248", "", r"010|0[0-2]", &[7]),
    c("SD", "249", "0", r"00", &[9]),
    c("SE", "46", "0", r"00", &[7, 8, 9]),
    c("SG", "65", "", r"0[0-3]\d", &[8]),
    c("SH", "290", "", r"00", &[4, 5]),
    c("SI", "386", "0", r"00|10(?:22|66|88|99)", &[8]),
    c("SJ", "47", "", r"00", &[8]),
    c("SK", "421", "0", r"00", &[6, 7, 9]),
    c("SL", "232", "0", r"00", &[8]),
    c("SM", "378", "", r"00", &[8, 10]),
    c("SN", "221", "", r"00", &[9]),
    c("SO", "252", "0", r"00", &[6, 7, 8, 9]),
    c("SR", "597", "", r"00", &[6, 7]),
    c("SS", "211", "0", r"00", &[9]),
    c("ST", "239", "", r"00", &[7]),
    c("SV", "503", "", r"00", &[8]),
    c("SX", "1", "", r"011", &[10]),
    c("SY", "963", "0", r"00", &[8, 9]),
    c("SZ", "268", "", r"00", &[8]),
    c("TC", "1", "", r"011", &[10]),
    c("TD", "235", "", r"00|16", &[8]),
    c("TG", "228", "", r"00", &[8]),
    c("TH", "66", "0", r"00[1-9]", &[8, 9]),
    c("TJ", "992", "", r"810", &[9]),
    c("TK", "690", "", r"00", &[4, 5, 6, 7]),
    c("TL", "670", "", r"00", &[7, 8]),
    c("TM", "993", "8", r"810", &[8]),
    c("TN", "216", "", r"00", &[8]),
    c("TO", "676", "", r"00", &[5, 7]),
    c("TR", "90", "0", r"00", &[10]),
    c("TT", "1", "", r"011", &[10]),
    c("TV", "688", "", r"00", &[5, 6, 7]),
    c("TW", "886", "0", r"0(?:0[25-79]|19)", &[8, 9]),
    c("TZ", "255", "0", r"00[056]", &[9]),
    c("UA", "380", "0", r"00", &[9]),
    c("UG", "256", "0", r"00[057]", &[9]),
    c("US", "1", "", r"011", &[10]),
    c("UY", "598", "0", r"0(?:0|1[3-9]\d)", &[8]),
    c("UZ", "998", "", r"00", &[9]),
    c("VA", "39", "", r"00", &[6, 7, 8, 9, 10, 11]),
    c("VC", "1", "", r"011", &[10]),
    c("VE", "58", "0", r"00", &[10]),
    c("VG", "1", "", r"011", &[10]),
    c("VI", "1", "", r"011", &[10]),
    c("VN", "84", "0", r"00", &[9, 10]),
    c("VU", "678", "", r"00", &[5, 7]),
    c("WF", "681", "", r"00", &[6]),
    c("WS", "685", "", r"0", &[5, 6, 7, 10]),
    c("XK", "383", "0", r"00", &[8, 9, 10, 11, 12]),
    c("YE", "967", "0", r"00", &[7, 8, 9]),
    c("YT", "262", "0", r"00", &[9]),
    c("ZA", "27", "0", r"00", &[5, 6, 7, 8, 9]),
    c("ZM", "260", "0", r"00", &[9]),
    c("ZW", "263", "0", r"00", &[7, 9]),
];

/// The country with ISO code `code`, in any case. `None` for a code the
/// table does not hold.
#[must_use]
pub fn country(code: &str) -> Option<&'static Country> {
    let code = code.trim();
    COUNTRIES
        .iter()
        .find(|country| country.code.eq_ignore_ascii_case(code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_code_is_listed_once_in_order() {
        let codes: Vec<&str> = COUNTRIES.iter().map(|c| c.code).collect();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(codes, sorted);
    }

    #[test]
    fn a_country_is_found_by_its_code_in_any_case() {
        assert_eq!(country("gb").map(|c| c.calling_code), Some("44"));
        assert_eq!(country(" US ").map(|c| c.calling_code), Some("1"));
        assert!(country("ZZ").is_none());
    }

    #[test]
    fn every_calling_code_is_digits() {
        let codes: HashSet<&str> = COUNTRIES.iter().map(|c| c.calling_code).collect();
        assert!(codes.iter().all(|c| c.chars().all(|d| d.is_ascii_digit())));
    }
}
