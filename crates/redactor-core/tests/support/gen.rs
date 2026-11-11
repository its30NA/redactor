//! Seeded generator for format-valid FAKE tokens.
//!
//! No realistic token is ever written into the repository: tests build them at run time from
//! a prefix plus a seeded random body, so secret scanners and push protection stay quiet.
//! Checksummed formats (GitHub CRC32, BIP-39, base58check, Luhn, IBAN) are computed here
//! independently of the engine's validators.

use sha2::{Digest, Sha256};

pub const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
pub const HEX: &[u8] = b"0123456789abcdef";
pub const HEX_UPPER: &[u8] = b"0123456789ABCDEF";
pub const LOWER_ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
pub const UPPER_ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
pub const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub const B64URL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
pub const B32: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
pub const B32_LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
pub const B58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
pub const DIGITS: &[u8] = b"0123456789";
pub const URL_PASSWORD: &[u8] =
    b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-._~!$&*+=%,;:@";
pub const CRYPT: &[u8] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

const WORDS: &str = include_str!("../../src/rules/bip39_english.txt");

/// SplitMix64: tiny, seeded, good enough for fake tokens.
pub struct Gen(u64);

impl Gen {
    pub fn new(seed: u64) -> Gen {
        Gen(seed)
    }

    /// A seed derived from a name (FNV-1a), so each test gets its own stable stream.
    pub fn named(name: &str) -> Gen {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in name.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        Gen(h)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next_u64() as u8).collect()
    }

    /// `n` characters from `alphabet`, regenerated until the classes in the alphabet that
    /// matter for "random-looking" checks (lower/upper/digit) are all present (n >= 8).
    pub fn from(&mut self, alphabet: &[u8], n: usize) -> String {
        let wanted = classes(alphabet);
        loop {
            let s: String = (0..n)
                .map(|_| alphabet[self.below(alphabet.len())] as char)
                .collect();
            if n < 8 || classes(s.as_bytes()) == wanted {
                return s;
            }
        }
    }

    pub fn alnum(&mut self, n: usize) -> String {
        self.from(BASE62, n)
    }

    pub fn hex(&mut self, n: usize) -> String {
        self.from(HEX, n)
    }

    pub fn digits(&mut self, n: usize) -> String {
        self.from(DIGITS, n)
    }

    /// GitHub-style token: prefix + 30 random base62 + 6-char base62 CRC32 of those 30.
    pub fn github(&mut self, prefix: &str) -> String {
        let random = self.alnum(30);
        format!(
            "{prefix}{random}{}",
            base62(crc32(random.as_bytes()) as u64, 6)
        )
    }

    /// A JWT with an HS256 header.
    pub fn jwt(&mut self) -> String {
        let header = b64url_encode(br#"{"alg":"HS256","typ":"JWT"}"#);
        let payload = format!(r#"{{"sub":"{}","iat":1700000000}}"#, self.alnum(12));
        let sig = self.bytes(32);
        format!(
            "{header}.{}.{}",
            b64url_encode(payload.as_bytes()),
            b64url_encode(&sig)
        )
    }

    /// A BIP-39 mnemonic; `valid == false` breaks the checksum.
    pub fn bip39(&mut self, words: usize, valid: bool) -> String {
        let list: Vec<&str> = WORDS.lines().collect();
        let ent_bits = words * 11 * 32 / 33;
        let entropy = self.bytes(ent_bits / 8);
        let hash = Sha256::digest(&entropy);
        let mut bits: Vec<bool> = Vec::new();
        for b in &entropy {
            for k in (0..8).rev() {
                bits.push((b >> k) & 1 == 1);
            }
        }
        for k in 0..(words * 11 - ent_bits) {
            bits.push((hash[k / 8] >> (7 - k % 8)) & 1 == 1);
        }
        let mut idx: Vec<usize> = bits
            .chunks(11)
            .map(|c| c.iter().fold(0usize, |n, &b| (n << 1) | b as usize))
            .collect();
        if !valid {
            // Flip the lowest checksum bit: same entropy, wrong checksum.
            let last = idx.len() - 1;
            idx[last] ^= 1;
        }
        idx.iter().map(|&i| list[i]).collect::<Vec<_>>().join(" ")
    }

    /// Bitcoin WIF (mainnet, compressed).
    pub fn wif(&mut self) -> String {
        let mut payload = vec![0x80u8];
        payload.extend(self.bytes(32));
        payload.push(0x01);
        base58check_encode(&payload)
    }

    /// A Luhn-valid card number with the given prefix and length.
    pub fn card(&mut self, prefix: &str, len: usize) -> String {
        let mut s = prefix.to_string();
        while s.len() < len - 1 {
            s.push((b'0' + self.below(10) as u8) as char);
        }
        for d in 0..10u8 {
            let candidate = format!("{s}{d}");
            if luhn(&candidate) {
                return candidate;
            }
        }
        unreachable!("one check digit always fits")
    }

    /// A mod-97 valid German IBAN (DE + 2 check digits + 18 digits).
    pub fn iban_de(&mut self) -> String {
        let bban = self.digits(18);
        // Check digits: 98 - (bban + "DE00" as digits) mod 97; D=13, E=14.
        let numeric = format!("{bban}131400");
        let rem = numeric
            .bytes()
            .fold(0u32, |r, b| (r * 10 + (b - b'0') as u32) % 97);
        format!("DE{:02}{bban}", 98 - rem)
    }
}

fn classes(s: &[u8]) -> u8 {
    let mut c = 0;
    if s.iter().any(u8::is_ascii_lowercase) {
        c |= 1;
    }
    if s.iter().any(u8::is_ascii_uppercase) {
        c |= 2;
    }
    if s.iter().any(u8::is_ascii_digit) {
        c |= 4;
    }
    c
}

/// Bitwise CRC32 (IEEE), deliberately not table-driven like the engine's.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

pub fn base62(mut n: u64, width: usize) -> String {
    let mut out = Vec::new();
    while n > 0 {
        out.push(BASE62[(n % 62) as usize]);
        n /= 62;
    }
    while out.len() < width {
        out.push(b'0');
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

fn b64_encode_with(data: &[u8], alphabet: &[u8], pad: bool) -> String {
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        let chars = chunk.len() + 1;
        for i in 0..4 {
            if i < chars {
                out.push(alphabet[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else if pad {
                out.push('=');
            }
        }
    }
    out
}

pub fn b64_encode(data: &[u8]) -> String {
    b64_encode_with(data, B64, true)
}

pub fn b64url_encode(data: &[u8]) -> String {
    b64_encode_with(data, B64URL, false)
}

pub fn base58check_encode(payload: &[u8]) -> String {
    let check = Sha256::digest(Sha256::digest(payload));
    let mut data = payload.to_vec();
    data.extend_from_slice(&check[..4]);
    let zeros = data.iter().take_while(|&&b| b == 0).count();
    let mut digits: Vec<u8> = Vec::new();
    for &byte in &data {
        let mut carry = byte as u32;
        for d in digits.iter_mut() {
            carry += (*d as u32) << 8;
            *d = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let mut s: String = "1".repeat(zeros);
    s.extend(digits.iter().rev().map(|&d| B58[d as usize] as char));
    s
}

/// Expands the body of a corpus `⟦gen:NAME⟧` marker: a format-valid fake token and the label
/// the engine should give it (`None`: any label). A `@b64`, `@url` or `@json` suffix encodes
/// the token (base64, percent-encoding, JSON string escapes); encoded tokens accept any label.
pub fn corpus_token(g: &mut Gen, name: &str) -> Option<(String, Option<&'static str>)> {
    if let Some((base, enc)) = name.split_once('@') {
        let (token, _) = corpus_token(g, base)?;
        let encoded = match enc {
            "b64" => b64_encode(token.as_bytes()),
            "url" => percent_encode(&token),
            "json" => token
                .replace('\\', "\\\\")
                .replace('\n', "\\n")
                .replace('"', "\\\""),
            _ => return None,
        };
        return Some((encoded, None));
    }
    let any = |t: String| Some((t, None));
    let l = |t: String, label: &'static str| Some((t, Some(label)));
    match name {
        // AI
        "openai" => l(format!("sk-proj-{}", g.alnum(60)), "OpenAI API Key"),
        "openai_legacy" => l(
            format!("sk-{}T3BlbkFJ{}", g.alnum(20), g.alnum(20)),
            "OpenAI API Key",
        ),
        "anthropic" => l(
            format!("sk-ant-api03-{}AA", g.alnum(93)),
            "Anthropic API Key",
        ),
        "huggingface" => l(format!("hf_{}", g.alnum(34)), "Hugging Face Token"),
        "groq" => l(format!("gsk_{}", g.alnum(52)), "Groq API Key"),
        "replicate" => l(format!("r8_{}", g.alnum(37)), "Replicate API Token"),
        "openrouter" => l(format!("sk-or-v1-{}", g.hex(64)), "OpenRouter API Key"),
        // Code hosting and developer tools
        "github_pat" => l(g.github("ghp_"), "GitHub Token"),
        "github_oauth" => l(g.github("gho_"), "GitHub OAuth Token"),
        "github_fine" => l(
            format!("github_pat_{}_{}", g.alnum(22), g.alnum(59)),
            "GitHub Token",
        ),
        "gitlab" => l(format!("glpat-{}", g.alnum(20)), "GitLab Token"),
        "npm" => l(format!("npm_{}", g.alnum(36)), "npm Access Token"),
        "pypi" => l(format!("pypi-Ag{}", g.alnum(90)), "PyPI API Token"),
        "dockerhub" => l(format!("dckr_pat_{}", g.alnum(27)), "Docker Hub Token"),
        "vault" => l(format!("hvs.{}", g.alnum(90)), "Vault Token"),
        "digitalocean" => l(format!("dop_v1_{}", g.hex(64)), "DigitalOcean Token"),
        "sentry" => l(format!("sntryu_{}", g.hex(64)), "Sentry Token"),
        "linear" => l(format!("lin_api_{}", g.alnum(40)), "Linear API Key"),
        "terraform" => l(
            format!("{}.atlasv1.{}", g.alnum(14), g.alnum(67)),
            "Terraform Cloud Token",
        ),
        // SaaS and messaging
        "slack_bot" => l(
            format!("xoxb-{}-{}-{}", g.digits(12), g.digits(13), g.alnum(24)),
            "Slack Token",
        ),
        "slack_client" => l(
            format!(
                "xoxc-{}-{}-{}-{}",
                g.digits(13),
                g.digits(13),
                g.digits(13),
                g.hex(128)
            ),
            "Slack Token",
        ),
        "databricks" => l(format!("dapi{}", g.hex(32)), "Databricks Token"),
        "elevenlabs" => l(format!("sk_{}", g.hex(48)), "ElevenLabs API Key"),
        "hubspot_pat" => l(
            format!(
                "pat-na1-{}-{}-{}-{}-{}",
                g.hex(8),
                g.hex(4),
                g.hex(4),
                g.hex(4),
                g.hex(12)
            ),
            "HubSpot Token",
        ),
        "slack_user" => l(
            format!(
                "xoxp-{}-{}-{}-{}",
                g.digits(12),
                g.digits(12),
                g.digits(12),
                g.hex(32)
            ),
            "Slack Token",
        ),
        "slack_webhook" => l(
            format!(
                "https://hooks.slack.com/services/T{}/B{}/{}",
                g.from(UPPER_ALNUM, 10),
                g.from(UPPER_ALNUM, 10),
                g.alnum(24)
            ),
            "Slack Webhook URL",
        ),
        "discord_webhook" => l(
            format!(
                "https://discord.com/api/webhooks/{}/{}",
                g.digits(18),
                g.alnum(68)
            ),
            "Discord Webhook URL",
        ),
        "teams_webhook" => l(
            format!(
                "https://example.webhook.office.com/webhookb2/{}-{}@{}/IncomingWebhook/{}/{}",
                g.hex(8),
                g.hex(12),
                g.hex(12),
                g.hex(32),
                g.hex(12)
            ),
            "Microsoft Teams Webhook URL",
        ),
        "stripe_test" => l(format!("sk_test_{}", g.alnum(30)), "Stripe Secret Key"),
        "stripe_live" => l(format!("sk_live_{}", g.alnum(30)), "Stripe Secret Key"),
        "stripe_webhook" => l(format!("whsec_{}", g.alnum(32)), "Stripe Webhook Secret"),
        "sendgrid" => l(
            format!("SG.{}.{}", g.alnum(22), g.alnum(43)),
            "SendGrid API Key",
        ),
        "mailgun" => l(format!("key-{}", g.hex(32)), "Mailgun API Key"),
        "twilio_sid" => l(format!("AC{}", g.hex(32)), "Twilio Account SID"),
        "twilio_key" => l(format!("SK{}", g.hex(32)), "Twilio API Key"),
        "telegram" => l(
            format!("{}:AA{}", g.digits(9), g.alnum(33)),
            "Telegram Bot Token",
        ),
        "shopify" => l(format!("shpat_{}", g.hex(32)), "Shopify Token"),
        "notion" => l(format!("ntn_{}", g.alnum(46)), "Notion Token"),
        "tailscale" => l(
            format!("tskey-auth-{}-{}", g.alnum(16), g.alnum(32)),
            "Tailscale Key",
        ),
        "age" => l(
            format!("AGE-SECRET-KEY-1{}", g.from(UPPER_ALNUM, 58)),
            "age Secret Key",
        ),
        // Cloud
        "aws_key_id" => l(format!("AKIA{}", g.from(B32, 16)), "AWS Access Key ID"),
        "aws_secret" => any(g.from(B64, 40)),
        "google_api" => l(format!("AIza{}", g.alnum(35)), "Google API Key"),
        "google_oauth" => l(format!("ya29.{}", g.alnum(120)), "Google OAuth Token"),
        "google_client_secret" => l(
            format!("GOCSPX-{}", g.alnum(28)),
            "Google OAuth Client Secret",
        ),
        "azure_storage_key" => any(format!("{}==", g.from(B64, 86))),
        "azure_ad_secret" => l(
            format!("{}{}Q~{}", g.alnum(3), g.digits(1), g.alnum(34)),
            "Azure AD Client Secret",
        ),
        // Keys and crypto material
        "jwt" => l(g.jwt(), "JSON Web Token"),
        "wif" => l(g.wif(), "Bitcoin Private Key"),
        "bip39_12" => l(g.bip39(12, true), "Recovery Phrase"),
        "bip39_24" => l(g.bip39(24, true), "Recovery Phrase"),
        "pem_rsa" => l(pem(g, "RSA PRIVATE KEY"), "Private Key"),
        "pem_ec" => l(pem(g, "EC PRIVATE KEY"), "Private Key"),
        "pem_pkcs8" => l(pem(g, "PRIVATE KEY"), "Private Key"),
        "pem_openssh" => l(pem(g, "OPENSSH PRIVATE KEY"), "Private Key"),
        "pgp_block" => l(
            format!(
                "-----BEGIN PGP PRIVATE KEY BLOCK-----\n\n{}\n{}\n={}\n-----END PGP PRIVATE KEY BLOCK-----",
                g.from(B64, 64),
                g.from(B64, 64),
                g.from(B64, 4)
            ),
            "PGP Private Key",
        ),
        "wireguard" => any(format!("{}A=", g.from(B64, 42))),
        "bcrypt" => l(format!("$2b$12${}", g.from(CRYPT, 53)), "Password Hash"),
        "sha512crypt" => l(
            format!("$6${}${}", g.from(CRYPT, 16), g.from(CRYPT, 86)),
            "Password Hash",
        ),
        "argon2" => l(
            format!(
                "$argon2id$v=19$m=65536,t=3,p=4${}${}",
                g.alnum(22),
                g.alnum(43)
            ),
            "Password Hash",
        ),
        "basic_auth" => any(b64_encode(format!("demo:{}", g.alnum(14)).as_bytes())),
        // Generic random values: the label comes from the context.
        "alnum16" => any(g.alnum(16)),
        "alnum20" => any(g.alnum(20)),
        "alnum24" => any(g.alnum(24)),
        "alnum32" => any(g.alnum(32)),
        "alnum40" => any(g.alnum(40)),
        "alnum64" => any(g.alnum(64)),
        "hex32" => any(g.hex(32)),
        "hex40" => any(g.hex(40)),
        "hex64" => any(g.hex(64)),
        "b64_24" => any(g.from(B64, 24)),
        "b64_44" => any(format!("{}=", g.from(B64, 43))),
        "b64_88" => any(format!("{}==", g.from(B64, 86))),
        "b32_32" => any(g.from(B32, 32)),
        // Long URL passwords: letters, digits and the punctuation a URL password may hold.
        "url_password_300" => any(g.from(URL_PASSWORD, 300)),
        "url_password_4k" => any(g.from(URL_PASSWORD, 4096)),
        _ => None,
    }
}

/// A PEM private key block with a random body.
fn pem(g: &mut Gen, kind: &str) -> String {
    format!(
        "-----BEGIN {kind}-----\n{}\n{}\n{}=\n-----END {kind}-----",
        g.from(B64, 64),
        g.from(B64, 64),
        g.from(B64, 31)
    )
}

/// Percent-encodes everything except unreserved characters.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn luhn(s: &str) -> bool {
    let sum: u32 = s
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, b)| {
            let d = (b - b'0') as u32;
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 {
                    x - 9
                } else {
                    x
                }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}
