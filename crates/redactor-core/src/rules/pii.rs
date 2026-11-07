//! Personal data. These rules only run when PII detection is on.

use super::{validate, Bound, Family, Hit, Rule, Verdict};

fn r(
    id: &'static str,
    label: &'static str,
    shape: &'static str,
    anchors: &'static [&'static str],
    re: &'static str,
    conf: f32,
) -> Rule {
    Rule::new(Family::Pii, id, label, shape, anchors, re, conf)
}

fn iban(h: &Hit) -> Verdict {
    if validate::iban_ok(h.secret()) {
        Verdict::Keep
    } else {
        Verdict::Reject
    }
}

/// Luhn plus a plausible issuer prefix (2-6: Mastercard, Visa, Amex, Discover, ...).
fn card(h: &Hit) -> Verdict {
    let digits: String = h.secret().chars().filter(char::is_ascii_digit).collect();
    let issuer = matches!(digits.as_bytes().first(), Some(b'2'..=b'6'));
    if issuer && (13..=19).contains(&digits.len()) && validate::luhn_ok(&digits) {
        Verdict::Keep
    } else {
        Verdict::Reject
    }
}

fn ipv4(h: &Hit) -> Verdict {
    let ok = h.secret().parse::<std::net::Ipv4Addr>().is_ok();
    // Not part of a longer dotted run (version strings like 1.2.3.4.5).
    let before = h.text[..h.start].chars().next_back();
    let after = h.text[h.end..].chars().next();
    let dotted = |c: Option<char>| matches!(c, Some('.'));
    if ok && !dotted(before) && !dotted(after) {
        Verdict::Keep
    } else {
        Verdict::Reject
    }
}

fn ipv6(h: &Hit) -> Verdict {
    let s = h.secret();
    let groups = s.split(':').filter(|g| !g.is_empty()).count();
    if groups >= 3 && s.parse::<std::net::Ipv6Addr>().is_ok() {
        Verdict::Keep
    } else {
        Verdict::Reject
    }
}

/// US SSN: area not 000, 666 or 9xx; group not 00; serial not 0000.
fn ssn(h: &Hit) -> Verdict {
    let s = h.secret();
    let (area, group, serial) = (&s[0..3], &s[4..6], &s[7..11]);
    if area == "000" || area == "666" || area.starts_with('9') || group == "00" || serial == "0000"
    {
        Verdict::Reject
    } else {
        Verdict::Keep
    }
}

pub(super) fn rules() -> Vec<Rule> {
    vec![
        r(
            "email",
            "Email Address",
            "local@domain.tld",
            &["@"],
            r"[A-Za-z0-9._%+-]{1,64}@[A-Za-z0-9-]{1,63}(?:\.[A-Za-z0-9-]{1,63})*\.[A-Za-z]{2,24}",
            0.9,
        )
        .back(64)
        .fwd(260),
        r(
            "phone_e164",
            "Phone Number",
            "+<country> <number> (E.164, optional spaces)",
            &[],
            r"\+[1-9][0-9]{0,3}[ .-]?\(?[0-9]{1,4}\)?(?:[ .-]?[0-9]{2,4}){2,4}",
            0.7,
        )
        .bounds(Bound::Word, Bound::Word),
        r(
            "phone_de",
            "Phone Number",
            "German numbers: 0xxx xxxxxxx, 0xxx/xxxxxxx, 0049 …",
            &[],
            r"(?:0049[ ]?(?:\(0\)[ ]?)?[1-9][0-9]{1,4}|0[1-9][0-9]{1,4})[ /-][0-9]{3,10}(?:[ -][0-9]{1,6})?",
            0.7,
        )
        .bounds(Bound::Word, Bound::Word),
        r(
            "iban",
            "IBAN",
            "CCkk + 11-30 alphanumerics (mod-97 valid)",
            &[],
            r"[A-Z]{2}[0-9]{2}(?: ?[A-Z0-9]{4}){2,7}(?: ?[A-Z0-9]{1,4})?",
            0.95,
        )
        .check(iban),
        r(
            "credit_card",
            "Credit Card Number",
            "13-19 digits, optional spaces/dashes (Luhn valid)",
            &[],
            r"[0-9](?:[ -]?[0-9]){12,18}",
            0.9,
        )
        .bounds(Bound::Word, Bound::Word)
        .check(card),
        r(
            "ipv4",
            "IP Address",
            "a.b.c.d",
            &[],
            r"(?:[0-9]{1,3}\.){3}[0-9]{1,3}",
            0.7,
        )
        .check(ipv4),
        r(
            "ipv6",
            "IP Address",
            "IPv6 address (3+ groups)",
            &[],
            r"[0-9A-Fa-f]{0,4}(?::[0-9A-Fa-f]{0,4}){2,7}",
            0.7,
        )
        .check(ipv6),
        r(
            "us_ssn",
            "US Social Security Number",
            "ddd-dd-dddd",
            &[],
            r"[0-9]{3}-[0-9]{2}-[0-9]{4}",
            0.75,
        )
        .bounds(Bound::Word, Bound::Word)
        .check(ssn),
    ]
}
