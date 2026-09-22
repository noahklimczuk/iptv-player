//! Which language a channel or title is in (README §7.3, "Hide non-[language]").
//!
//! Providers do not agree on how to say this. A name carries `FR |`, `[AR]`, `US:`,
//! `(SPANISH)`, or nothing at all; a group title carries `ARABIC | MOVIES` or
//! `VOD - Français`; `tvg-language` carries `English`, `eng`, `en`, or is absent. So
//! the answer comes from several signals, tried most-trustworthy first, and it is
//! allowed to be "no idea".
//!
//! That last part matters more than the detection does. An unknown language must stay
//! unknown, because a filter that treats unknown as non-English would empty the
//! library of every provider that tags nothing — which is most of them.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::title;

/// Tokens that name a language, mapped to ISO 639-1.
///
/// Both the language's own names and the country codes that imply it, because playlists
/// use them interchangeably: `FR |` is as likely to mean French as `FRENCH -` is.
const TOKENS: &[(&str, &str)] = &[
    // English. "UK" is the United Kingdom here, not Ukrainian: no playlist in the wild
    // uses the ISO 639-1 meaning, and Ukrainian arrives as "UKR" or "UKRAINE".
    ("en", "en"),
    ("eng", "en"),
    ("english", "en"),
    ("anglais", "en"),
    ("uk", "en"),
    ("gb", "en"),
    ("gbr", "en"),
    ("britain", "en"),
    ("british", "en"),
    ("us", "en"),
    ("usa", "en"),
    ("america", "en"),
    ("american", "en"),
    ("au", "en"),
    ("aus", "en"),
    ("australia", "en"),
    ("nz", "en"),
    ("newzealand", "en"),
    ("ie", "en"),
    ("irl", "en"),
    ("ireland", "en"),
    ("irish", "en"),
    ("za", "en"),
    ("ca", "en"),
    ("can", "en"),
    ("canada", "en"),
    // Arabic. "AR" is Arabic in playlists; Argentina arrives as "ARG".
    ("ar", "ar"),
    ("ara", "ar"),
    ("arabic", "ar"),
    ("arab", "ar"),
    ("arabia", "ar"),
    ("sa", "ar"),
    ("ksa", "ar"),
    ("ae", "ar"),
    ("uae", "ar"),
    ("eg", "ar"),
    ("egypt", "ar"),
    ("ma", "ar"),
    ("dz", "ar"),
    ("tn", "ar"),
    ("iq", "ar"),
    ("jo", "ar"),
    ("kw", "ar"),
    ("qa", "ar"),
    ("lb", "ar"),
    ("ly", "ar"),
    ("om", "ar"),
    ("ye", "ar"),
    ("sy", "ar"),
    ("bh", "ar"),
    ("sd", "ar"),
    ("mar", "ar"),
    ("morocco", "ar"),
    ("algeria", "ar"),
    // French
    ("fr", "fr"),
    ("fra", "fr"),
    ("fre", "fr"),
    ("french", "fr"),
    ("francais", "fr"),
    ("france", "fr"),
    ("vf", "fr"),
    ("vostfr", "fr"),
    ("vfq", "fr"),
    ("quebec", "fr"),
    // Spanish
    ("es", "es"),
    ("esp", "es"),
    ("spa", "es"),
    ("spanish", "es"),
    ("espanol", "es"),
    ("spain", "es"),
    ("espana", "es"),
    ("latino", "es"),
    ("lat", "es"),
    ("latin", "es"),
    ("mx", "es"),
    ("mex", "es"),
    ("mexico", "es"),
    ("arg", "es"),
    ("argentina", "es"),
    ("chile", "es"),
    ("colombia", "es"),
    ("peru", "es"),
    ("venezuela", "es"),
    ("castellano", "es"),
    // German
    ("de", "de"),
    ("deu", "de"),
    ("ger", "de"),
    ("german", "de"),
    ("deutsch", "de"),
    ("germany", "de"),
    ("deutschland", "de"),
    ("at", "de"),
    ("aut", "de"),
    ("austria", "de"),
    ("osterreich", "de"),
    // Italian
    ("it", "it"),
    ("ita", "it"),
    ("italian", "it"),
    ("italiano", "it"),
    ("italy", "it"),
    ("italia", "it"),
    // Portuguese
    ("pt", "pt"),
    ("por", "pt"),
    ("portuguese", "pt"),
    ("portugues", "pt"),
    ("portugal", "pt"),
    ("br", "pt"),
    ("bra", "pt"),
    ("brazil", "pt"),
    ("brasil", "pt"),
    // Dutch
    ("nl", "nl"),
    ("nld", "nl"),
    ("dut", "nl"),
    ("dutch", "nl"),
    ("nederlands", "nl"),
    ("holland", "nl"),
    ("netherlands", "nl"),
    ("nederland", "nl"),
    // Nordic
    ("se", "sv"),
    ("swe", "sv"),
    ("sv", "sv"),
    ("swedish", "sv"),
    ("sverige", "sv"),
    ("no", "no"),
    ("nor", "no"),
    ("norwegian", "no"),
    ("norge", "no"),
    ("dk", "da"),
    ("dan", "da"),
    ("da", "da"),
    ("danish", "da"),
    ("danmark", "da"),
    ("fi", "fi"),
    ("fin", "fi"),
    ("finnish", "fi"),
    ("suomi", "fi"),
    ("is", "is"),
    ("isl", "is"),
    ("iceland", "is"),
    ("icelandic", "is"),
    // Eastern Europe
    ("ru", "ru"),
    ("rus", "ru"),
    ("russian", "ru"),
    ("russia", "ru"),
    ("ukr", "uk"),
    ("ukraine", "uk"),
    ("ukrainian", "uk"),
    ("pl", "pl"),
    ("pol", "pl"),
    ("polish", "pl"),
    ("polska", "pl"),
    ("cz", "cs"),
    ("cze", "cs"),
    ("ces", "cs"),
    ("czech", "cs"),
    ("cesko", "cs"),
    ("sk", "sk"),
    ("svk", "sk"),
    ("slovak", "sk"),
    ("slovensko", "sk"),
    ("hu", "hu"),
    ("hun", "hu"),
    ("hungarian", "hu"),
    ("magyar", "hu"),
    ("magyarorszag", "hu"),
    ("ro", "ro"),
    ("rou", "ro"),
    ("rom", "ro"),
    ("romanian", "ro"),
    ("romania", "ro"),
    ("bg", "bg"),
    ("bul", "bg"),
    ("bulgarian", "bg"),
    ("bulgaria", "bg"),
    ("rs", "sr"),
    ("srb", "sr"),
    ("serbian", "sr"),
    ("srbija", "sr"),
    ("hr", "hr"),
    ("hrv", "hr"),
    ("croatian", "hr"),
    ("hrvatska", "hr"),
    ("ba", "bs"),
    ("bih", "bs"),
    ("bosnian", "bs"),
    ("exyu", "sr"),
    ("si", "sl"),
    ("svn", "sl"),
    ("slovenian", "sl"),
    ("al", "sq"),
    ("alb", "sq"),
    ("albanian", "sq"),
    ("shqip", "sq"),
    ("mk", "mk"),
    ("mkd", "mk"),
    ("macedonian", "mk"),
    ("lt", "lt"),
    ("lithuanian", "lt"),
    ("lv", "lv"),
    ("latvian", "lv"),
    ("ee", "et"),
    ("est", "et"),
    ("estonian", "et"),
    ("gr", "el"),
    ("ell", "el"),
    ("gre", "el"),
    ("greek", "el"),
    ("greece", "el"),
    ("ellada", "el"),
    ("hellas", "el"),
    // Middle East and Central Asia
    ("il", "he"),
    ("heb", "he"),
    ("hebrew", "he"),
    ("israel", "he"),
    ("ir", "fa"),
    ("fa", "fa"),
    ("fas", "fa"),
    ("per", "fa"),
    ("farsi", "fa"),
    ("persian", "fa"),
    ("iran", "fa"),
    ("tr", "tr"),
    ("tur", "tr"),
    ("turkish", "tr"),
    ("turkiye", "tr"),
    ("turkce", "tr"),
    ("kurdish", "ku"),
    ("kurdi", "ku"),
    ("kurd", "ku"),
    ("az", "az"),
    ("aze", "az"),
    ("azeri", "az"),
    ("ge", "ka"),
    ("geo", "ka"),
    ("georgian", "ka"),
    ("am", "hy"),
    ("arm", "hy"),
    ("armenian", "hy"),
    ("kz", "kk"),
    ("kaz", "kk"),
    ("kazakh", "kk"),
    ("uz", "uz"),
    ("uzbek", "uz"),
    ("af", "ps"),
    ("pashto", "ps"),
    ("afghan", "ps"),
    // South Asia
    ("hi", "hi"),
    ("hin", "hi"),
    ("hindi", "hi"),
    ("in", "hi"),
    ("ind", "hi"),
    ("india", "hi"),
    ("desi", "hi"),
    ("pk", "ur"),
    ("urdu", "ur"),
    ("pakistan", "ur"),
    ("bd", "bn"),
    ("ben", "bn"),
    ("bengali", "bn"),
    ("bangla", "bn"),
    ("tamil", "ta"),
    ("tam", "ta"),
    ("telugu", "te"),
    ("tel", "te"),
    ("malayalam", "ml"),
    ("kannada", "kn"),
    ("marathi", "mr"),
    ("punjabi", "pa"),
    ("gujarati", "gu"),
    ("np", "ne"),
    ("nepali", "ne"),
    ("sinhala", "si"),
    // East and Southeast Asia
    ("cn", "zh"),
    ("chi", "zh"),
    ("zho", "zh"),
    ("chinese", "zh"),
    ("china", "zh"),
    ("mandarin", "zh"),
    ("cantonese", "zh"),
    ("hk", "zh"),
    ("tw", "zh"),
    ("taiwan", "zh"),
    ("jp", "ja"),
    ("jpn", "ja"),
    ("japanese", "ja"),
    ("japan", "ja"),
    ("kr", "ko"),
    ("kor", "ko"),
    ("korean", "ko"),
    ("korea", "ko"),
    ("th", "th"),
    ("tha", "th"),
    ("thai", "th"),
    ("thailand", "th"),
    ("vn", "vi"),
    ("vie", "vi"),
    ("vietnamese", "vi"),
    ("vietnam", "vi"),
    ("indonesian", "id"),
    ("indonesia", "id"),
    ("bahasa", "id"),
    ("malay", "ms"),
    ("malaysia", "ms"),
    ("ph", "tl"),
    ("phi", "tl"),
    ("tagalog", "tl"),
    ("filipino", "tl"),
    ("kh", "km"),
    ("khmer", "km"),
    ("lao", "lo"),
    ("mm", "my"),
    ("burmese", "my"),
    // Africa
    ("afrikaans", "af"),
    ("swahili", "sw"),
    ("amharic", "am"),
    ("somali", "so"),
    ("ng", "en"),
    ("nigeria", "en"),
    ("ke", "sw"),
    ("kenya", "sw"),
];

fn table() -> &'static HashMap<&'static str, &'static str> {
    static T: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    T.get_or_init(|| TOKENS.iter().copied().collect())
}

/// The language a run of text is written in, judged by its script.
///
/// A name in Arabic or Han characters is not English whatever it is tagged, and this is
/// the one signal a provider cannot get wrong. Only counts when most of the letters
/// agree, so `"MBC مصر"` and `"Al Jazeera"` both land where they should.
fn script_language(text: &str) -> Option<&'static str> {
    let mut latin = 0usize;
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    for ch in text.chars() {
        if !ch.is_alphabetic() {
            continue;
        }
        let script = match ch as u32 {
            0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => "ar",
            0x0590..=0x05FF => "he",
            0x0400..=0x04FF => "ru",
            0x0370..=0x03FF => "el",
            0x0530..=0x058F => "hy",
            0x10A0..=0x10FF => "ka",
            0x0900..=0x097F => "hi",
            0x0980..=0x09FF => "bn",
            0x0B80..=0x0BFF => "ta",
            0x0E00..=0x0E7F => "th",
            0x1200..=0x137F => "am",
            0x3040..=0x30FF => "ja",
            0xAC00..=0xD7AF | 0x1100..=0x11FF => "ko",
            0x4E00..=0x9FFF | 0x3400..=0x4DBF => "zh",
            _ => {
                latin += 1;
                continue;
            }
        };
        *counts.entry(script).or_default() += 1;
    }
    let (script, n) = counts.into_iter().max_by_key(|&(_, n)| n)?;
    // Japanese writes Han alongside kana; a stray kana character is the giveaway.
    if n * 2 > latin {
        Some(script)
    } else {
        None
    }
}

/// Pull language tokens out of a name: the leading country/language prefix, and any
/// word inside brackets or between pipes.
///
/// Deliberately not every word — `"The German Doctor"` is an English-language film, and
/// a bare word in the middle of a title proves nothing.
fn tagged_tokens(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let (_, Some(code)) = title::split_country_prefix(name) {
        out.push(code.to_lowercase());
    }
    let mut segment = String::new();
    let mut inside = false;
    for ch in name.chars() {
        match ch {
            '(' | '[' | '{' | '|' => {
                if inside && !segment.trim().is_empty() {
                    out.push(segment.trim().to_lowercase());
                }
                segment.clear();
                inside = true;
            }
            ')' | ']' | '}' => {
                if !segment.trim().is_empty() {
                    out.push(segment.trim().to_lowercase());
                }
                segment.clear();
                inside = false;
            }
            _ if inside => segment.push(ch),
            _ => {}
        }
    }
    if inside && !segment.trim().is_empty() {
        out.push(segment.trim().to_lowercase());
    }
    out
}

/// Every word of a group title, which unlike a programme name exists to categorize.
fn group_tokens(group: &str) -> Vec<String> {
    group
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

fn lookup(token: &str) -> Option<&'static str> {
    let folded = title::fold_diacritics(token).to_lowercase();
    let cleaned: String = folded.chars().filter(|c| c.is_alphanumeric()).collect();
    table().get(cleaned.as_str()).copied()
}

/// Work out the language of one entry, or admit that the name does not say.
///
/// `reported` is whatever the provider put in `tvg-language`; `group` is its
/// `group-title`. Signals are tried in order of how much they can be trusted, and the
/// first that answers wins.
pub fn detect(name: &str, group: Option<&str>, reported: Option<&str>) -> Option<String> {
    // 1. What the provider says outright.
    if let Some(reported) = reported {
        for token in reported.split(|c: char| !c.is_alphanumeric()) {
            if let Some(code) = lookup(token) {
                return Some(code.to_string());
            }
        }
    }
    // 2. What the name is written in. A script cannot be mislabelled.
    if let Some(code) = script_language(name) {
        return Some(code.to_string());
    }
    // 3. Tags in the name: a prefix, or a bracketed or pipe-delimited segment.
    for token in tagged_tokens(name) {
        if let Some(code) = lookup(&token) {
            return Some(code.to_string());
        }
    }
    // 4. The group, which exists to categorize and so is safe to read word by word.
    if let Some(group) = group {
        if let Some(code) = script_language(group) {
            return Some(code.to_string());
        }
        for token in group_tokens(group) {
            if let Some(code) = lookup(&token) {
                return Some(code.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(name: &str) -> Option<String> {
        detect(name, None, None)
    }

    #[test]
    fn a_reported_language_wins_over_everything() {
        assert_eq!(
            detect("FR | TF1", None, Some("English")).as_deref(),
            Some("en")
        );
        assert_eq!(detect("TF1", None, Some("fra")).as_deref(), Some("fr"));
        assert_eq!(detect("TF1", None, Some("fr-FR")).as_deref(), Some("fr"));
    }

    #[test]
    fn a_prefix_names_the_language() {
        assert_eq!(d("FR | TF1 HD").as_deref(), Some("fr"));
        assert_eq!(d("[DE] RTL").as_deref(), Some("de"));
        assert_eq!(d("US: CNN").as_deref(), Some("en"));
        assert_eq!(d("UK - Sky Sports").as_deref(), Some("en"));
        assert_eq!(d("AR| MBC 1").as_deref(), Some("ar"));
    }

    #[test]
    fn a_bracketed_tag_anywhere_counts() {
        assert_eq!(d("Le Fabuleux Destin (FRENCH)").as_deref(), Some("fr"));
        assert_eq!(d("Parasite [KOREAN] 1080p").as_deref(), Some("ko"));
        assert_eq!(d("Das Boot | GERMAN |").as_deref(), Some("de"));
    }

    #[test]
    fn a_script_settles_it_whatever_the_tags_say() {
        assert_eq!(d("قناة الجزيرة").as_deref(), Some("ar"));
        assert_eq!(d("Первый канал").as_deref(), Some("ru"));
        assert_eq!(d("ΕΡΤ1").as_deref(), Some("el"));
        assert_eq!(d("NHK 総合").as_deref(), Some("zh").or(Some("ja")));
        assert_eq!(d("KBS 한국방송").as_deref(), Some("ko"));
    }

    #[test]
    fn a_group_answers_when_the_name_does_not() {
        assert_eq!(
            detect("MBC 1", Some("ARABIC | ENTERTAINMENT"), None).as_deref(),
            Some("ar")
        );
        assert_eq!(
            detect("Canal+", Some("VOD - Français"), None).as_deref(),
            Some("fr")
        );
        assert_eq!(detect("Rai 1", Some("Italia"), None).as_deref(), Some("it"));
    }

    #[test]
    fn a_name_that_says_nothing_stays_unknown() {
        // The whole filter depends on this: guessing here empties a library.
        assert_eq!(d("CNN"), None);
        assert_eq!(d("Sky Sports Main Event"), None);
        assert_eq!(d("Discovery Channel HD"), None);
        assert_eq!(d("The Matrix (1999) 1080p"), None);
    }

    #[test]
    fn quality_and_service_tags_in_prefix_position_are_not_languages() {
        // "VIP| CNN" and "4K - Sky" are extremely common, and neither names a language.
        assert_eq!(d("VIP| CNN"), None);
        assert_eq!(d("HD | Discovery"), None);
        assert_eq!(d("4K - Sky Atlantic"), None);
        assert_eq!(d("24/7 | The Office"), None);
        assert_eq!(d("PPV: Boxing Night"), None);
    }

    #[test]
    fn an_english_word_inside_a_title_is_not_a_tag() {
        // The trap: these are English-language films with a language word in the title.
        assert_eq!(d("The German Doctor"), None);
        assert_eq!(d("The French Connection (1971)"), None);
        assert_eq!(d("Spanish Affair"), None);
        assert_eq!(d("Polish Wedding"), None);
    }

    #[test]
    fn ambiguous_two_letter_codes_follow_playlist_convention() {
        // AR is Arabic in a playlist; Argentina writes itself out.
        assert_eq!(d("AR | Al Arabiya").as_deref(), Some("ar"));
        assert_eq!(d("ARG | TyC Sports").as_deref(), Some("es"));
        // UK is the United Kingdom, not Ukrainian.
        assert_eq!(d("UK| BBC One").as_deref(), Some("en"));
        assert_eq!(d("UKR | 1+1").as_deref(), Some("uk"));
    }

    #[test]
    fn diacritics_and_case_do_not_matter() {
        assert_eq!(d("[ESPAÑOL] Antena 3").as_deref(), Some("es"));
        assert_eq!(d("(português) Globo").as_deref(), Some("pt"));
        assert_eq!(d("[TÜRKÇE] TRT 1").as_deref(), Some("tr"));
    }

    #[test]
    fn an_empty_or_punctuation_name_is_not_a_crash() {
        assert_eq!(d(""), None);
        assert_eq!(d("---"), None);
        assert_eq!(detect("", Some(""), Some("")), None);
    }

    /// A slice of playlist shaped like the real thing, so a change to one token cannot
    /// quietly move a dozen others.
    #[test]
    fn a_realistic_playlist_lands_where_it_should() {
        let cases: &[(&str, Option<&str>, Option<&str>)] = &[
            ("UK| BBC One HD", Some("UK | ENTERTAINMENT"), Some("en")),
            ("US| ESPN 1", Some("USA | SPORTS"), Some("en")),
            ("CA| TSN 1 FHD", Some("CANADA"), Some("en")),
            ("IE | RTE One", None, Some("en")),
            ("FR| TF1 4K", Some("FRANCE | TNT"), Some("fr")),
            ("DE| Sky Sport 1", Some("DEUTSCHLAND"), Some("de")),
            ("ES| Movistar LaLiga", Some("ESPANA"), Some("es")),
            ("IT| Rai 1 HD", Some("ITALIA"), Some("it")),
            ("PT| SIC Noticias", Some("PORTUGAL"), Some("pt")),
            ("NL| NPO 1", None, Some("nl")),
            ("PL| TVP 1", Some("POLSKA"), Some("pl")),
            ("RO| Digi Sport 1", None, Some("ro")),
            ("TR| beIN Sports 1", Some("TURKIYE"), Some("tr")),
            ("AR| MBC Masr", Some("ARABIC"), Some("ar")),
            ("قناة أبوظبي الرياضية", None, Some("ar")),
            ("RU| Матч ТВ", None, Some("ru")),
            ("IN| Star Plus", Some("INDIA | HINDI"), Some("hi")),
            ("PK| Geo News", None, Some("ur")),
            ("Netflix | The Crown S04", Some("VOD | SERIES"), None),
            ("Sky Cinema Premiere HD", Some("MOVIES"), None),
            ("24/7 | Friends", Some("24/7 CHANNELS"), None),
            ("VIP | PPV 01", Some("PPV EVENTS"), None),
            ("The Matrix (1999) 1080p", Some("VOD | ACTION"), None),
            (
                "Le Fabuleux Destin d Amelie Poulain (FRENCH) 1080p",
                None,
                Some("fr"),
            ),
        ];
        let mut wrong = Vec::new();
        for (name, group, want) in cases {
            let got = detect(name, *group, None);
            if got.as_deref() != *want {
                wrong.push(format!("{name:?} -> {got:?}, wanted {want:?}"));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn a_latin_name_with_one_foreign_character_stays_latin() {
        // One stray glyph should not outvote the rest of the name.
        assert_eq!(d("Canal+ Sport ★"), None);
        assert_eq!(detect("MBC مصر", None, None).as_deref(), Some("ar"));
    }
}
