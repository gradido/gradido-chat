//! Search inside a thread: over what is loaded, without the server.

/// Fewer letters match too much.
pub const NEEDLE_MIN_CHARS: usize = 2;
/// How far back a search loads.
pub const SEARCH_MAX_ITEMS: usize = 1000;

/// Lower case and without accents, each letter with the byte it came from.
fn folded(text: &str) -> Vec<(char, usize)> {
    let mut out = Vec::new();
    for (at, c) in text.char_indices() {
        // Combining marks (U+0300..U+036F) are what NFD leaves of an accent;
        // precomposed letters fall back to their base letter below.
        if ('\u{300}'..='\u{36f}').contains(&c) {
            continue;
        }
        for lower in base_letter(c).to_lowercase() {
            out.push((lower, at));
        }
    }
    out
}

/// The unaccented letter for the accented Latin ones the ten languages use.
/// A full NFD table would cost more than the search is worth.
fn base_letter(c: char) -> char {
    const TABLE: &[(&str, char)] = &[
        ("àáâãäåāă", 'a'),
        ("ÀÁÂÃÄÅĀĂ", 'A'),
        ("çćč", 'c'),
        ("ÇĆČ", 'C'),
        ("èéêëēėę", 'e'),
        ("ÈÉÊËĒĖĘ", 'E'),
        ("ìíîïīı", 'i'),
        ("ÌÍÎÏĪİ", 'I'),
        ("ñń", 'n'),
        ("ÑŃ", 'N'),
        ("òóôõöōő", 'o'),
        ("ÒÓÔÕÖŌŐ", 'O'),
        ("ùúûüūű", 'u'),
        ("ÙÚÛÜŪŰ", 'U'),
        ("ýÿ", 'y'),
        ("Ý", 'Y'),
        ("şš", 's'),
        ("ŞŠ", 'S'),
        ("ğ", 'g'),
        ("Ğ", 'G'),
        ("άα", 'α'),
        ("έε", 'ε'),
        ("ήη", 'η'),
        ("ίϊΐι", 'ι'),
        ("όο", 'ο'),
        ("ύϋΰυ", 'υ'),
        ("ώω", 'ω'),
        ("й", 'и'),
        ("ё", 'е'),
    ];
    TABLE.iter().find(|(from, _)| from.contains(c)).map_or(c, |(_, to)| *to)
}

pub fn needle(raw: &str) -> String {
    let folded: String = folded(raw.trim()).into_iter().map(|(c, _)| c).collect();
    if folded.chars().count() < NEEDLE_MIN_CHARS { String::new() } else { folded }
}

/// Byte ranges of `text` that match the (already folded) needle, not overlapping.
pub fn ranges(text: &str, needle: &str) -> Vec<(usize, usize)> {
    let wanted: Vec<char> = needle.chars().collect();
    if wanted.is_empty() {
        return Vec::new();
    }
    let hay = folded(text);
    let mut out = Vec::new();
    let mut i = 0;
    while i + wanted.len() <= hay.len() {
        if hay[i..i + wanted.len()].iter().map(|(c, _)| c).eq(wanted.iter()) {
            let from = hay[i].1;
            let to = hay.get(i + wanted.len()).map_or(text.len(), |(_, at)| *at);
            if to > from {
                out.push((from, to));
            }
            i += wanted.len();
        } else {
            i += 1;
        }
    }
    out
}

pub fn matches(text: &str, needle: &str) -> bool {
    !ranges(text, needle).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_and_case_do_not_matter() {
        assert_eq!(ranges("Schönes Café", &needle("CAFE")), vec![(9, 14)]);
        assert_eq!(ranges("Schönes Café", &needle("schon")), vec![(0, 6)]);
    }

    #[test]
    fn one_letter_finds_nothing() {
        assert_eq!(needle(" a "), "");
        assert!(ranges("aaa", "").is_empty());
    }

    #[test]
    fn hits_do_not_overlap() {
        assert_eq!(ranges("aaaa", "aa"), vec![(0, 2), (2, 4)]);
    }
}
