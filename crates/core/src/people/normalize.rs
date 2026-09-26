//! Turning handles into comparable identities. Two cards are the same person when
//! they share a match key; names never count, since many people share one.

use hearth_protocol::Channel;

/// Cleans a value for display and storage, e.g. strips `mailto:` or `tel:`.
pub fn clean(channel: Channel, value: &str) -> String {
    let v = value.trim();
    match channel {
        Channel::Email => strip_prefix_ci(v, "mailto:").trim().to_owned(),
        Channel::Phone | Channel::Signal | Channel::Whatsapp => {
            strip_prefix_ci(v, "tel:").trim().to_owned()
        }
        Channel::Telegram => telegram_username(v)
            .map(|u| format!("@{u}"))
            .unwrap_or_else(|| v.to_owned()),
        Channel::Matrix | Channel::Other => v.to_owned(),
    }
}

/// The identity used to unify people across sources, or `None` when the value can't
/// safely identify anyone (too short, no digits…).
///
/// Phone-like channels share one key space (`phone:`): a Signal or WhatsApp number is a
/// phone number. Emails compare case-insensitively. Telegram usernames are unique.
pub fn match_key(channel: Channel, value: &str) -> Option<String> {
    match channel {
        Channel::Email => email_key(value).map(|e| format!("email:{e}")),
        Channel::Phone | Channel::Signal | Channel::Whatsapp => {
            phone_key(value).map(|p| format!("phone:{p}"))
        }
        Channel::Telegram => telegram_username(value).map(|u| format!("telegram:{u}")),
        Channel::Matrix => {
            let v = value.trim().to_lowercase();
            (v.starts_with('@') && v.contains(':')).then(|| format!("matrix:{v}"))
        }
        Channel::Other => None,
    }
}

fn email_key(value: &str) -> Option<String> {
    let v = strip_prefix_ci(value.trim(), "mailto:")
        .trim()
        .to_lowercase();
    let (local, domain) = v.split_once('@')?;
    (!local.is_empty() && domain.contains('.') && !v.contains(char::is_whitespace)).then_some(v)
}

/// Digits with a leading `+` for international numbers; `00` means `+`. National
/// numbers keep their digits as they are: guessing a country code would merge the
/// wrong people.
pub fn phone_key(value: &str) -> Option<String> {
    let v = strip_prefix_ci(value.trim(), "tel:");
    // Drop extensions ("…;ext=12", "… x12") before reading digits.
    let v = v.split([';', ',']).next().unwrap_or("");
    let v = v.split(" x").next().unwrap_or("");
    let international = v.trim_start().starts_with('+');
    let digits: String = v.chars().filter(char::is_ascii_digit).collect();
    let (plus, digits) = if international {
        (true, digits)
    } else if let Some(rest) = digits.strip_prefix("00") {
        (true, rest.to_owned())
    } else {
        (false, digits)
    };
    if digits.len() < 6 {
        return None;
    }
    Some(if plus { format!("+{digits}") } else { digits })
}

/// `@name`, `name`, `https://t.me/name`, `tg://resolve?domain=name` → `name`.
pub fn telegram_username(value: &str) -> Option<String> {
    let v = value.trim();
    let v = v
        .strip_prefix("https://")
        .or_else(|| v.strip_prefix("http://"))
        .unwrap_or(v);
    let v = v
        .strip_prefix("t.me/")
        .or_else(|| v.strip_prefix("telegram.me/"))
        .or_else(|| v.strip_prefix("tg://resolve?domain="))
        .or_else(|| strip_prefix_opt_ci(v, "telegram:"))
        .unwrap_or(v);
    let v = v
        .trim_start_matches('@')
        .split(['/', '?'])
        .next()
        .unwrap_or("");
    let valid = (4..=32).contains(&v.len())
        && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && v.starts_with(|c: char| c.is_ascii_alphabetic());
    valid.then(|| v.to_lowercase())
}

/// Names compared for "possible duplicates": case, accents and spacing ignored.
pub fn name_key(name: &str) -> String {
    let folded: String = name
        .to_lowercase()
        .chars()
        .map(fold_accent)
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}

fn strip_prefix_ci<'a>(v: &'a str, prefix: &str) -> &'a str {
    strip_prefix_opt_ci(v, prefix).unwrap_or(v)
}

fn strip_prefix_opt_ci<'a>(v: &'a str, prefix: &str) -> Option<&'a str> {
    (v.len() >= prefix.len() && v[..prefix.len()].eq_ignore_ascii_case(prefix))
        .then(|| &v[prefix.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phones() {
        assert_eq!(
            phone_key("+41 79 123 45 67").as_deref(),
            Some("+41791234567")
        );
        assert_eq!(
            phone_key("0041 (79) 123-45-67").as_deref(),
            Some("+41791234567")
        );
        assert_eq!(
            phone_key("tel:+41-79-123-45-67").as_deref(),
            Some("+41791234567")
        );
        // National numbers stay national: no guessing the country.
        assert_eq!(phone_key("079 123 45 67").as_deref(), Some("0791234567"));
        assert_ne!(phone_key("079 123 45 67"), phone_key("+41 79 123 45 67"));
        assert_eq!(
            phone_key("+1 555 010 9999;ext=12").as_deref(),
            Some("+15550109999")
        );
        assert_eq!(phone_key("112"), None);
        assert_eq!(phone_key("call me"), None);
    }

    #[test]
    fn emails() {
        assert_eq!(
            match_key(Channel::Email, " Sam.Carter@Example.COM ").as_deref(),
            Some("email:sam.carter@example.com")
        );
        assert_eq!(
            match_key(Channel::Email, "mailto:sam@example.com").as_deref(),
            Some("email:sam@example.com")
        );
        assert_eq!(match_key(Channel::Email, "not an email"), None);
        assert_eq!(
            clean(Channel::Email, "MAILTO:sam@example.com"),
            "sam@example.com"
        );
    }

    #[test]
    fn phone_like_channels_share_keys() {
        assert_eq!(
            match_key(Channel::Signal, "+41 79 123 45 67"),
            match_key(Channel::Phone, "0041791234567")
        );
    }

    #[test]
    fn telegram() {
        for v in [
            "@SamCarter",
            "samcarter",
            "https://t.me/samcarter",
            "tg://resolve?domain=samcarter",
            "telegram:samcarter",
        ] {
            assert_eq!(telegram_username(v).as_deref(), Some("samcarter"), "{v}");
        }
        assert_eq!(telegram_username("@ab"), None);
        assert_eq!(clean(Channel::Telegram, "t.me/SamCarter"), "@samcarter");
    }

    #[test]
    fn names() {
        assert_eq!(name_key("  Zoë  Müller "), name_key("zoe muller"));
        assert_ne!(name_key("Sam Carter"), name_key("Sam Carver"));
    }
}
