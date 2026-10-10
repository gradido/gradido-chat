//! Who somebody is on the screen when there is no picture: two letters on a colour.
//! Same rules as the web (avatarLettering.js, avatarColor.js, chatGroupDisplay.js).

pub const PALETTE: [u32; 10] = [
    0x4A5568, 0x2C7A7B, 0x805AD5, 0xDD6B20, 0x3182CE, 0x38A169, 0xE53E3E, 0x6B46C1, 0x2B6CB0, 0x9C4221,
];

pub fn member_letters(alias: &str) -> String {
    alias.chars().filter(char::is_ascii_alphanumeric).take(2).collect::<String>().to_uppercase()
}

/// An alias shorter than three letters is none: the id stands in.
pub fn member_name<'a>(alias: Option<&'a str>, gradido_id: &'a str) -> &'a str {
    match alias {
        Some(alias) if alias.chars().count() >= 3 => alias,
        _ => gradido_id,
    }
}

pub fn group_letters(title: &str) -> String {
    let words: Vec<String> = title
        .split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|w| !w.is_empty())
        .collect();
    let letters: String = match words.as_slice() {
        [] => String::new(),
        [one] => one.chars().take(2).collect(),
        [first, second, ..] => first.chars().take(1).chain(second.chars().take(1)).collect(),
    };
    letters.to_uppercase()
}

fn hashed(seed: &str) -> usize {
    let mut hash: i32 = 0;
    for unit in seed.encode_utf16() {
        hash = (unit as i32).wrapping_add(hash.wrapping_shl(5).wrapping_sub(hash));
    }
    (hash % 10).unsigned_abs() as usize
}

/// The server's index when it sent one, else a colour that stays with the seed.
pub fn color(index: Option<i64>, seed: &str) -> u32 {
    match index {
        Some(i) if (0..10).contains(&i) => PALETTE[i as usize],
        _ => PALETTE[hashed(seed)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters() {
        assert_eq!(member_letters("b.bloxberg"), "BB");
        assert_eq!(group_letters("Gradido-Café Berlin"), "GB");
        assert_eq!(group_letters("stammtisch"), "ST");
        assert_eq!(member_name(Some("ab"), "id-1"), "id-1");
    }

    #[test]
    fn colour_stays_with_the_seed() {
        assert_eq!(color(Some(3), "x"), PALETTE[3]);
        assert_eq!(color(None, "GB"), color(None, "GB"));
    }
}
