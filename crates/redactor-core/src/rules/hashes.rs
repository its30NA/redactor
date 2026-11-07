//! Password hashes. A leaked hash can be cracked offline, so it counts as a secret.

use super::{Bound, Family, Rule};

fn r(
    id: &'static str,
    label: &'static str,
    shape: &'static str,
    anchors: &'static [&'static str],
    re: &'static str,
    conf: f32,
) -> Rule {
    Rule::new(Family::Hash, id, label, shape, anchors, re, conf)
}

pub(super) fn rules() -> Vec<Rule> {
    vec![
        r(
            "bcrypt_hash",
            "Password Hash",
            "$2a$/$2b$/$2y$ + cost + 53 chars",
            &["$2a$", "$2b$", "$2y$", "$2x$"],
            r"\$2[abxy]\$[0-9]{2}\$[./A-Za-z0-9]{53,}",
            0.95,
        ),
        r(
            "argon2_hash",
            "Password Hash",
            "$argon2id$v=19$m=…,t=…,p=…$salt$hash",
            &["$argon2"],
            r"\$argon2(?:id|i|d)\$v=[0-9]+\$m=[0-9]+,t=[0-9]+,p=[0-9]+\$[A-Za-z0-9+/]+\$[A-Za-z0-9+/]+",
            0.95,
        ),
        r(
            "sha512_crypt_hash",
            "Password Hash",
            "$6$[rounds=N$]salt$ + 86 chars",
            &["$6$"],
            r"\$6\$(?:rounds=[0-9]+\$)?[./A-Za-z0-9]{1,16}\$[./A-Za-z0-9]{86,}",
            0.95,
        ),
        r(
            "sha256_crypt_hash",
            "Password Hash",
            "$5$[rounds=N$]salt$ + 43 chars",
            &["$5$"],
            r"\$5\$(?:rounds=[0-9]+\$)?[./A-Za-z0-9]{1,16}\$[./A-Za-z0-9]{43,}",
            0.95,
        ),
        r(
            "md5_crypt_hash",
            "Password Hash",
            "$1$salt$ + 22 chars",
            &["$1$"],
            r"\$1\$[./A-Za-z0-9]{1,8}\$[./A-Za-z0-9]{22,}",
            0.95,
        ),
        r(
            "apr1_hash",
            "Password Hash",
            "$apr1$salt$ + 22 chars",
            &["$apr1$"],
            r"\$apr1\$[./A-Za-z0-9]{1,8}\$[./A-Za-z0-9]{22,}",
            0.95,
        ),
        r(
            "yescrypt_hash",
            "Password Hash",
            "$y$params$salt$ + 43 chars",
            &["$y$"],
            r"\$y\$[./A-Za-z0-9]+\$[./A-Za-z0-9]{1,86}\$[./A-Za-z0-9]{43,}",
            0.95,
        ),
        r(
            "pbkdf2_django_hash",
            "Password Hash",
            "pbkdf2_sha256$iterations$salt$hash",
            &["pbkdf2_sha256$", "pbkdf2_sha1$"],
            r"pbkdf2_sha(?:256|1)\$[0-9]{4,7}\$[A-Za-z0-9./+]{1,64}\$[A-Za-z0-9+/=]{20,}",
            0.95,
        ),
        r(
            "pwdump_hash",
            "Windows Password Hash",
            "user:rid:<LM 32 hex>:<NT 32 hex>:::",
            &[":::"],
            r"[^\s:]{1,64}:[0-9]{1,10}:(?P<s>[0-9A-Fa-f]{32}:[0-9A-Fa-f]{32}):::",
            0.95,
        )
        .back(180)
        .fwd(3)
        .bounds(Bound::None, Bound::None),
        r(
            "htpasswd_sha",
            "Password Hash",
            "user:{SHA}<28 base64>",
            &[":{SHA}"],
            r"[^\s:]{1,64}:(?P<s>\{SHA\}[A-Za-z0-9+/]{27,}=?)",
            0.95,
        )
        .back(64)
        .fwd(40)
        .bounds(Bound::None, Bound::Base64),
    ]
}
