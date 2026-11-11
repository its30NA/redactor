//! The context engine through the full engine: the 22 v1 probe cases, the false
//! positives that make people distrust such tools, and the code-versus-config line.
//!
//! Every value here is fake. Vendor-shaped tokens are generated at run time.

mod support;

use std::time::Instant;

use redactor_core::{Engine, EngineConfig, ScanOutcome, ScanRequest, Sensitivity, Source};
use support::gen::*;

fn engine() -> Engine {
    Engine::new(EngineConfig::default()).unwrap()
}

fn scan_at(text: &str, s: Sensitivity) -> ScanOutcome {
    engine().scan(
        text,
        &ScanRequest {
            sensitivity: Some(s),
            ..ScanRequest::default()
        },
    )
}

/// At Balanced, exactly the bytes of `secret` (its first occurrence) are one finding.
fn assert_redacted(text: &str, secret: &str) {
    assert_redacted_range(
        text,
        text.find(secret).expect("text holds the secret"),
        secret.len(),
    );
}

fn assert_redacted_range(text: &str, pos: usize, len: usize) {
    let out = scan_at(text, Sensitivity::Balanced);
    let spans: Vec<(usize, usize, &str)> = out
        .findings
        .iter()
        .map(|f| (f.start, f.end, f.detector.as_str()))
        .collect();
    assert!(
        out.findings
            .iter()
            .any(|f| !f.kept && f.start == pos && f.end == pos + len),
        "{text:?}: expected [{pos}, {}) redacted, got {spans:?}",
        pos + len
    );
    // The value is gone from the output (when it occurs once; repeats need propagation).
    let value = &text[pos..pos + len];
    if text.matches(value).count() == 1 {
        assert!(
            !out.output.contains(value),
            "{text:?}: the value survived: {:?}",
            out.output
        );
    }
}

/// Nothing at all is found, at Balanced and at Paranoid.
fn assert_clean(text: &str) {
    for s in [Sensitivity::Balanced, Sensitivity::Paranoid] {
        let out = scan_at(text, s);
        let spans: Vec<(&str, &str)> = out
            .findings
            .iter()
            .map(|f| (&text[f.start..f.end], f.detector.as_str()))
            .collect();
        assert!(out.findings.is_empty(), "{text:?} at {s:?}: {spans:?}");
    }
}

// ---------------------------------------------------------------------------------------
// The 22 v1 probe cases (v1 missed 17 of them)
// ---------------------------------------------------------------------------------------

#[test]
fn probe_json_quoted_key() {
    assert_redacted(
        r#"{"password": "Hunter2-Fake-9x", "user": "demo"}"#,
        "Hunter2-Fake-9x",
    );
}

#[test]
fn probe_python_dict() {
    assert_redacted(
        "creds = {'api_key': 'Zq81mXv0pLr3TfK9wQe2'}",
        "Zq81mXv0pLr3TfK9wQe2",
    );
}

#[test]
fn probe_yaml_spaces() {
    assert_redacted(
        r#"password: "correct horse battery staple""#,
        "correct horse battery staple",
    );
}

#[test]
fn probe_german_passwort() {
    assert_redacted(
        "Mein Passwort ist Tr0ub4dor&3 bitte nicht teilen",
        "Tr0ub4dor&3",
    );
}

#[test]
fn probe_prose_pw_is() {
    assert_redacted("the wifi pw is Kx9!mPq2vL - keep it safe", "Kx9!mPq2vL");
}

#[test]
fn probe_curl_u() {
    assert_redacted(
        "curl -u admin:Sup3rS3cret! https://api.internal/v1",
        "Sup3rS3cret!",
    );
}

#[test]
fn probe_mysql_p() {
    assert_redacted("mysql -u root -pS3cretDbPass9 prod", "S3cretDbPass9");
}

#[test]
fn probe_password_flag() {
    assert_redacted("deploy --password Xy7#kLm2Qp --region eu", "Xy7#kLm2Qp");
}

#[test]
fn probe_xml_element() {
    assert_redacted("<password>Fake-Xml-Pass-77</password>", "Fake-Xml-Pass-77");
}

#[test]
fn probe_netrc() {
    assert_redacted(
        "machine api.example.org login demo password N3trcFakePw",
        "N3trcFakePw",
    );
}

#[test]
fn probe_adonet() {
    assert_redacted(
        "Server=db;Database=app;User Id=sa;Password=Ad0N3tFake!;",
        "Ad0N3tFake!",
    );
}

#[test]
fn probe_pgp_private_block() {
    let mut g = Gen::named("probe pgp");
    let block = format!(
        "-----BEGIN PGP PRIVATE KEY BLOCK-----\n\n{}\n={}\n-----END PGP PRIVATE KEY BLOCK-----",
        g.from(B64, 64),
        g.from(B64, 4)
    );
    assert_redacted(&block, &block);
}

#[test]
fn probe_telegram_bot_token() {
    let mut g = Gen::named("probe telegram");
    let token = format!("{}:AA{}", g.digits(9), g.alnum(33));
    assert_redacted(&format!("BOT={token}"), &token);
}

#[test]
fn probe_tailscale_key() {
    let mut g = Gen::named("probe tailscale");
    let key = format!("tskey-auth-{}-{}", g.alnum(16), g.alnum(32));
    assert_redacted(&format!("TS_AUTHKEY {key}"), &key);
}

#[test]
fn probe_groq_key() {
    let mut g = Gen::named("probe groq");
    let key = format!("gsk_{}", g.alnum(52));
    assert_redacted(&key, &key);
}

#[test]
fn probe_x_api_key_header() {
    assert_redacted(
        "X-Api-Key: 9f8e7d6c5b4a39281706f5e4d3c2b1a0",
        "9f8e7d6c5b4a39281706f5e4d3c2b1a0",
    );
}

/// The first occurrence is caught by its key; the second one needs propagation.
#[test]
fn probe_secret_reused_later_first_occurrence() {
    assert_redacted(
        "DB_PASSWORD=Hunter2-Fake-9x\n... login failed for Hunter2-Fake-9x",
        "Hunter2-Fake-9x",
    );
}

#[test]
fn probe_secret_reused_later() {
    let text = "DB_PASSWORD=Hunter2-Fake-9x\n... login failed for Hunter2-Fake-9x";
    let out = scan_at(text, Sensitivity::Balanced);
    assert!(!out.output.contains("Hunter2-Fake-9x"), "{}", out.output);
    let second = text.rfind("Hunter2-Fake-9x").unwrap();
    assert!(out
        .findings
        .iter()
        .any(|f| f.start == second && f.source == Source::Propagated));
}

#[test]
fn probe_zero_width_split_key() {
    let mut g = Gen::named("probe zero width");
    let token = g.github("ghp_");
    let text = format!("key: {}\u{200B}{}", &token[..17], &token[17..]);
    // The finding covers the token and the hidden character inside it.
    assert_redacted_range(&text, 5, token.len() + '\u{200B}'.len_utf8());
}

#[test]
fn probe_k8s_base64() {
    assert_redacted(
        "data:\n  password: SHVudGVyMi1GYWtlLTl4",
        "SHVudGVyMi1GYWtlLTl4",
    );
}

#[test]
fn probe_query_param_token() {
    assert_redacted(
        "GET /cb?access_token=Zq81mXv0pLr3TfK9wQe2Lm&state=1",
        "Zq81mXv0pLr3TfK9wQe2Lm",
    );
}

#[test]
fn probe_env_hash_in_quotes() {
    assert_redacted(r#"DB_PASSWORD="p#ss-Fake-91""#, "p#ss-Fake-91");
}

#[test]
fn probe_powershell_securestring() {
    assert_redacted(
        r#"ConvertTo-SecureString "PsFake!Pass42" -AsPlainText -Force"#,
        "PsFake!Pass42",
    );
}

// ---------------------------------------------------------------------------------------
// Code or config: the same line shape, decided by the syntax around it
// ---------------------------------------------------------------------------------------

#[test]
fn env_unquoted_identifier_is_a_secret() {
    assert_redacted("DB_PASSWORD=hunter2prod\n", "hunter2prod");
}

#[test]
fn env_value_repeated_in_prose_is_still_a_secret() {
    assert_redacted(
        "export DB_PASSWORD=hunter2prod\nlogin failed for hunter2prod\n",
        "hunter2prod",
    );
}

#[test]
fn ini_spaced_value_is_a_secret() {
    assert_redacted("[database]\npassword = x9k2prod\n", "x9k2prod");
}

/// A plain word or a classic weak password as an INI value at the start of a line.
#[test]
fn ini_plain_word_is_a_secret() {
    assert_redacted("[database]\ndb_password = dragonfly\n", "dragonfly");
    assert_redacted("[mail]\nsmtp_password = password1\n", "password1");
    // Code keeps its variables.
    assert_clean("password = password_input\n");
    assert_clean("def f(user, prompt):\n    password = prompt\n    return check(user, password)\n");
}

#[test]
fn yaml_plain_word_is_a_secret() {
    assert_redacted("db:\n  password: hunterprod\n", "hunterprod");
}

#[test]
fn properties_value_is_a_secret() {
    assert_redacted("spring.datasource.password=hunter2prod\n", "hunter2prod");
}

#[test]
fn quoted_literal_in_code_is_a_secret() {
    assert_redacted("    password = \"Hunter2-Fake-9x\"\n", "Hunter2-Fake-9x");
    assert_redacted("const token = 'Fake-Tok-77x';\n", "Fake-Tok-77x");
}

#[test]
fn plain_word_for_a_machine_secret_only_in_paranoid() {
    // Tokens, keys and secrets are generated, not words: a quoted word or identifier there
    // is a fixture name or a UI string. It stays a Paranoid finding.
    for (text, value) in [
        ("token1: \"Hello\"\n", "Hello"),
        ("{'token': 'ticketBlob'}\n", "ticketBlob"),
        ("\"secret\": \"ticketBlob\"\n", "ticketBlob"),
        ("apiKey = 'ticketBlob'\n", "ticketBlob"),
    ] {
        assert!(
            scan_at(text, Sensitivity::Balanced).findings.is_empty(),
            "{text:?}"
        );
        let out = scan_at(text, Sensitivity::Paranoid);
        assert!(!out.output.contains(value), "{text:?}: {}", out.output);
    }
    // People do pick words as passwords, and generated values stay certain.
    assert_redacted("password: 'ticketBlob'\n", "ticketBlob");
    assert_redacted("token1: \"Fake-Tok-77x\"\n", "Fake-Tok-77x");
}

#[test]
fn code_identifier_is_not_a_secret() {
    assert_clean("password = password_input\n");
}

#[test]
fn code_identifier_used_elsewhere_is_not_a_secret() {
    assert_clean("def login(user, x9k2):\n    password = x9k2\n    return check(user, password)\n");
}

#[test]
fn code_declaration_identifier_is_not_a_secret() {
    assert_clean("let password = userpw2;\n");
}

// ---------------------------------------------------------------------------------------
// Required negatives: substring keys
// ---------------------------------------------------------------------------------------

fn substring_key_never_fires(word: &str) {
    // A 20-character random value is a standalone classifier finding whatever its key; the
    // key itself must not make it a context finding.
    let text = format!("{word}=Zq81mXv0pLr3TfK9wQe2\n");
    for s in [Sensitivity::Balanced, Sensitivity::Paranoid] {
        let out = scan_at(&text, s);
        assert!(
            out.findings
                .iter()
                .all(|f| f.source != Source::Context && f.label != "Password"),
            "{text:?} at {s:?}: {:?}",
            out.findings
        );
    }
    for text in [
        format!("{word}: Xy7#kLm2Qp\n"),
        format!("the {word} is Kx9!mPq2vL\n"),
        format!("run --{word} Xy7#kLm2Qp\n"),
        format!("\"{word}\": \"Hunter2-Fake-9x\"\n"),
    ] {
        assert_clean(&text);
    }
}

macro_rules! substring_cases {
    ($($name:ident $word:literal;)*) => {
        $(
            #[test]
            fn $name() {
                substring_key_never_fires($word);
            }
        )*
    };
}

substring_cases! {
    substring_key_bypass "bypass";
    substring_key_passport "passport";
    substring_key_compass "compass";
    substring_key_keyboard "keyboard";
    substring_key_monkey "monkey";
    substring_key_turkey "turkey";
    substring_key_tokenizer "tokenizer";
    substring_key_spinner "spinner";
    substring_key_pinned "pinned";
    substring_key_ping "ping";
}

// ---------------------------------------------------------------------------------------
// Required negatives: one test per line
// ---------------------------------------------------------------------------------------

macro_rules! clean_cases {
    ($($name:ident $text:expr;)*) => {
        $(
            #[test]
            fn $name() {
                assert_clean($text);
            }
        )*
    };
}

clean_cases! {
    // Negative modifiers
    negative_max_tokens "max_tokens: 4096\n";
    negative_token_count "token_count=12\n";
    negative_token_type "token_type: Bearer\n";
    negative_password_min_length "password_min_length: 12\n";
    negative_password_reset_url "PASSWORD_RESET_URL=https://example.org/reset\n";
    negative_secret_name "secret_name: db-creds\n";
    negative_public_key "public_key: ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeFakeFakeFakeFakeFakeFakeFake demo@laptop\n";
    negative_key_size "key_size=2048\n";
    // Type annotations and declarations
    type_password_string "password: string\n";
    type_token_string "  token: String,\n";
    type_api_key_optional "  apiKey?: string;\n";
    type_secret_str "    secret: &str,\n";
    type_password_option "    password: Option<String>,\n";
    type_def_login "def login(user, password=None):\n";
    type_store_add "function storeAdd(secret: string, label: string) {}\n";
    // Expressions as values
    expr_getpass "password = getpass()\n";
    expr_getenv "token = os.getenv(\"TOKEN\")\n";
    expr_config_member "api_key=config.api_key\n";
    expr_request_form "password = request.form['password']\n";
    expr_process_env "  secret: process.env.SECRET,\n";
    expr_self_assign "        self.password = password\n";
    expr_await "const token = await getToken()\n";
    // Prose
    prose_password_required "the password is required";
    prose_token_expired "your token has expired";
    prose_passwort_falsch "Passwort ist falsch";
    prose_passwort_vergessen "Passwort vergessen?";
    prose_enter_pin "Enter your PIN";
    prose_reset_password_link "reset password link";
    prose_token_based_auth "token-based auth";
    prose_api_key_rotation "API key rotation policy";
}

// ---------------------------------------------------------------------------------------
// More syntaxes and languages
// ---------------------------------------------------------------------------------------

#[test]
fn multilingual_keywords() {
    for (text, value) in [
        ("رمز عبور: Xy7#kLm2Qp", "Xy7#kLm2Qp"),
        ("mot de passe : Xy7#kLm2Qp", "Xy7#kLm2Qp"),
        ("la contraseña es Xy7#kLm2Qp", "Xy7#kLm2Qp"),
        ("пароль: Xy7#kLm2Qp", "Xy7#kLm2Qp"),
        ("Пароль Xy7#kLm2Qp", "Xy7#kLm2Qp"),
        ("密码是Xy7kLm2Qp9", "Xy7kLm2Qp9"),
        ("パスワード: Xy7#kLm2Qp", "Xy7#kLm2Qp"),
        ("Kennwort: Xy7#kLm2Qp", "Xy7#kLm2Qp"),
        ("hasło: Xy7#kLm2Qp", "Xy7#kLm2Qp"),
        ("Wi-Fi şifre: Xy7#kLm2Qp", "Xy7#kLm2Qp"),
    ] {
        assert_redacted(text, value);
    }
}

#[test]
fn command_lines() {
    for (text, value) in [
        ("sshpass -p Fake-Ssh-Pw9 ssh me@host", "Fake-Ssh-Pw9"),
        (
            "ldapsearch -x -D cn=admin -w Fake-Ldap-9 -b dc=x",
            "Fake-Ldap-9",
        ),
        (
            "docker login -u me -p Fake-Dock-9x registry.example.org",
            "Fake-Dock-9x",
        ),
        ("zip -P Fake-Zip-9x out.zip notes.txt", "Fake-Zip-9x"),
        ("7z a -pFake-7z-9x out.7z notes", "Fake-7z-9x"),
        ("sqlplus scott/Fake-Tiger9@orcl", "Fake-Tiger9"),
        (
            "CREATE USER app IDENTIFIED BY 'Fake-Sql-9x';",
            "Fake-Sql-9x",
        ),
        ("ALTER ROLE app WITH PASSWORD 'Fake-Pg-9x';", "Fake-Pg-9x"),
        (
            "net use Z: \\\\srv\\share Fake-Net-9x /user:corp\\me",
            "Fake-Net-9x",
        ),
        (
            "cmdkey /generic:host /user:me /pass:Fake-Cmd-9x",
            "Fake-Cmd-9x",
        ),
        (
            "keytool -list -keystore k.jks -storepass Fake-Store-9x",
            "Fake-Store-9x",
        ),
        (
            "openssl pkcs12 -in a.p12 -passin pass:Fake-Ossl-9x",
            "Fake-Ossl-9x",
        ),
        (
            "gpg --batch --passphrase Fake-Gpg-9x -d f.gpg",
            "Fake-Gpg-9x",
        ),
        (
            "kubectl create secret generic db --from-literal=password=Fake-K8s-9x",
            "Fake-K8s-9x",
        ),
        (
            "redis-cli config set requirepass Fake-Redis-9x",
            "Fake-Redis-9x",
        ),
    ] {
        assert_redacted(text, value);
    }
}

#[test]
fn config_syntaxes() {
    for (text, value) in [
        ("export API_KEY=Fake-Api-Key-9x\n", "Fake-Api-Key-9x"),
        ("set PASSWORD=Fake-Win-9x\n", "Fake-Win-9x"),
        ("$env:API_TOKEN = \"Fake-Ps-Tok-9x\"\n", "Fake-Ps-Tok-9x"),
        ("ENV DB_PASSWORD Fake-Docker-9x\n", "Fake-Docker-9x"),
        (
            "<add key=\"DbPassword\" value=\"Fake-Cfg-9x\"/>\n",
            "Fake-Cfg-9x",
        ),
        ("password := \"Fake-Go-9x\"\n", "Fake-Go-9x"),
        ("{ :password => 'Fake-Rb-9x' }\n", "Fake-Rb-9x"),
        (
            "    Key Content            : Fake-Wifi-9x\n",
            "Fake-Wifi-9x",
        ),
        (
            "client_secret: Fake-Client-Secret-9x\n",
            "Fake-Client-Secret-9x",
        ),
        ("PGPASSWORD=Fake-Pg-9x psql -h db\n", "Fake-Pg-9x"),
        (
            "db_password: hunter-prod # rotated monthly\n",
            "hunter-prod",
        ),
        ("pin: 4711\n", "4711"),
    ] {
        assert_redacted(text, value);
    }
}

#[test]
fn context_findings_carry_their_source_and_label() {
    let out = scan_at("DB_PASSWORD=Fake-Pw-9x\n", Sensitivity::Balanced);
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].source, Source::Context);
    assert_eq!(out.findings[0].label, "Password");
    assert_eq!(out.output, "DB_PASSWORD=[REDACTED: Password]\n");
}

#[test]
fn context_can_be_disabled() {
    let e = Engine::new(EngineConfig {
        disabled: vec!["context".into()],
        ..EngineConfig::default()
    })
    .unwrap();
    let out = e.scan("DB_PASSWORD=Fake-Pw-9x\n", &ScanRequest::default());
    assert!(out.findings.is_empty());
}

#[test]
fn prose_plain_word_only_in_paranoid() {
    let text = "the password is hunterprod";
    assert!(scan_at(text, Sensitivity::Balanced).findings.is_empty());
    let out = scan_at(text, Sensitivity::Paranoid);
    assert_eq!(out.output, "the password is [REDACTED: Password]");
}

/// Numbers count for PIN keys; for a password only with 6+ digits on a config line or in a
/// JSON string (`DB_PASSWORD=12345678`, see `digit_short_and_letterless_passwords`).
#[test]
fn numbers_only_for_pins() {
    assert_clean("timeout: 30\npassword: 2024\n");
    assert_clean("LED_PIN = 13\n");
    assert_clean("password_hash_rounds: 120000\n");
}

#[test]
fn placeholders_and_templates_are_not_values() {
    for text in [
        "DB_PASSWORD=changeme\n",
        "password: ${DB_PASSWORD}\n",
        "token: \"{{ vault_token }}\"\n",
        "API_KEY=your_api_key_here\n",
        "--password $PW\n",
        "password: ********\n",
        "private_key: /etc/ssl/private/server.key\n",
    ] {
        assert_clean(text);
    }
}

#[test]
fn i18n_strings_are_not_values() {
    for text in [
        "\"password\": \"Passwort\",\n",
        "\"password\": \"Enter your password\",\n",
        "password: Mot de passe\n",
        "\"token\": \"Token\",\n",
    ] {
        assert_clean(text);
    }
}

// ---------------------------------------------------------------------------------------
// Where an unquoted value ends: `&` and `;` only before the next pair, and a line that is
// only an assignment keeps its blanks
// ---------------------------------------------------------------------------------------

/// At Balanced and at Paranoid, exactly the bytes of `secret` (its first occurrence) are
/// redacted: one finding covers them, no finding reaches past them, and they are gone.
fn assert_only(text: &str, secret: &str) {
    let pos = text.find(secret).expect("text holds the secret");
    let end = pos + secret.len();
    for s in [Sensitivity::Balanced, Sensitivity::Paranoid] {
        let out = scan_at(text, s);
        let spans: Vec<(&str, &str)> = out
            .findings
            .iter()
            .filter(|f| !f.kept)
            .map(|f| (&text[f.start..f.end], f.detector.as_str()))
            .collect();
        let hit = out
            .findings
            .iter()
            .any(|f| !f.kept && f.start == pos && f.end == end);
        let inside = out
            .findings
            .iter()
            .filter(|f| !f.kept)
            .all(|f| f.start >= pos && f.end <= end);
        assert!(
            hit && inside,
            "{text:?} at {s:?}: expected only {secret:?}, got {spans:?}"
        );
        assert!(
            !out.output.contains(secret),
            "{text:?} at {s:?}: survived: {:?}",
            out.output
        );
    }
}

#[test]
fn amp_inside_env_value_belongs_to_it() {
    assert_only("DB_PASSWORD=p@ss&word!\n", "p@ss&word!");
}

#[test]
fn amp_inside_short_key_value_belongs_to_it() {
    assert_only("pwd=Tr0ub4dor&3\n", "Tr0ub4dor&3");
}

#[test]
fn amp_inside_set_value_belongs_to_it() {
    assert_only("set PASSWORD=Pa$$w0rd&1\n", "Pa$$w0rd&1");
}

#[test]
fn amp_inside_export_value_belongs_to_it() {
    assert_only("export API_TOKEN=abc&def123XYZ\n", "abc&def123XYZ");
}

#[test]
fn amp_joined_words_are_one_value() {
    assert_only("password=correct&horse\n", "correct&horse");
}

#[test]
fn amp_before_next_query_pair_ends_the_value() {
    assert_only(
        "https://example.org/login?user=bob&password=hunter2&next=/home",
        "hunter2",
    );
}

#[test]
fn amp_before_next_pair_ends_a_generated_key() {
    let key = format!("sk_test_{}", Gen::named("amp-key").alnum(24));
    assert_only(&format!("api_key={key}&x=1\n"), &key);
}

#[test]
fn amp_entity_before_next_pair_ends_the_value() {
    assert_only(
        "<a href=\"/x?password=Hunter2-Fake-9x&amp;next=1\">",
        "Hunter2-Fake-9x",
    );
}

#[test]
fn semicolon_before_next_pair_ends_a_connection_string_value() {
    assert_only("Server=x;Password=Abc&123;Database=y", "Abc&123");
}

#[test]
fn semicolon_starting_a_line_pair_ends_the_value() {
    assert_only("Password=Abc&123;Database=y\n", "Abc&123");
}

#[test]
fn semicolon_inside_a_value_belongs_to_it() {
    assert_only("DB_PASSWORD=Fake;9xQ2\n", "Fake;9xQ2");
}

#[test]
fn comment_after_value_is_not_part_of_it() {
    assert_only("password=hunter2 # my pw\n", "hunter2");
}

#[test]
fn ini_semicolon_comment_is_not_part_of_the_value() {
    assert_only("password = Hunter2-Fake-9x ; old one\n", "Hunter2-Fake-9x");
}

#[test]
fn dotenv_quoted_value_with_spaces() {
    assert_only("REPORT_PASSWORD='my dog Rex 2019'\n", "my dog Rex 2019");
    assert_only(
        "SMTP_PASSWORD=\"blue monkey 42 rides\"\n",
        "blue monkey 42 rides",
    );
    assert_only("export SECRET_KEY='Winter2024!'\n", "Winter2024!");
    assert_clean("LOGIN_MESSAGE=\"Please enter your password.\"\n");
}

#[test]
fn dotenv_value_with_spaces_runs_to_the_line_end() {
    assert_only("PASSWORD=pass word with spaces\n", "pass word with spaces");
}

#[test]
fn dotenv_value_with_spaces_drops_trailing_blanks_and_comment() {
    assert_only(
        "DB_PASSWORD=Fake 9x Prod   # rotated monthly\n",
        "Fake 9x Prod",
    );
}

#[test]
fn yaml_passphrase_is_redacted_in_full() {
    assert_only(
        "password: correct horse battery staple\n",
        "correct horse battery staple",
    );
}

#[test]
fn shell_prefix_assignment_ends_at_the_blank() {
    assert_only("PGPASSWORD=s3cr3t psql -h db -U app\n", "s3cr3t");
}

#[test]
fn shell_operator_after_value_ends_it_at_the_blank() {
    assert_only("password=Foo-9x && ./run.sh\n", "Foo-9x");
    // `foo` itself is too short for a lowercase `key=word` line; the command stays visible.
    let text = "password=foo && ./run.sh\n";
    let out = scan_at(text, Sensitivity::Balanced);
    assert!(
        out.output.ends_with("&& ./run.sh\n"),
        "{text:?}: {:?}",
        out.output
    );
}

#[test]
fn logfmt_fields_after_value_end_it_at_the_blank() {
    assert_only(
        "token=Zq81mXv0pLr3TfK9wQe2 user=demo status=200\n",
        "Zq81mXv0pLr3TfK9wQe2",
    );
}

#[test]
fn code_keeps_the_first_token() {
    assert_clean("password = getpass() or default\n");
    assert_clean("self.password = password_input if ok else None\n");
}

// ---------------------------------------------------------------------------------------
// Performance (release:`cargo test -p redactor-core --release --test context -- --ignored --nocapture`)
// ---------------------------------------------------------------------------------------

#[test]
#[ignore]
fn perf_password_lines() {
    let e = engine();
    for (name, text) in [
        ("100k empty password= lines", "password=\n".repeat(100_000)),
        (
            "100k password=<value> lines",
            "password=Hunter2-Fake-9x\n".repeat(100_000),
        ),
        (
            "100k mixed config lines",
            "DB_PASSWORD=hunter2prod\npassword = x9k2prod\nself.password = password\n"
                .repeat(34_000),
        ),
    ] {
        let t = Instant::now();
        let out = e.scan(&text, &ScanRequest::default());
        println!(
            "{name}: {:.1} ms ({} findings)",
            t.elapsed().as_secs_f64() * 1000.0,
            out.findings.len()
        );
    }
}

/// A URL value belongs to the URL detectors: only the userinfo password goes, the user,
/// host, port and database stay readable, and the same password elsewhere shares its
/// placeholder.
#[test]
fn url_value_redacts_only_the_password() {
    let url = "postgres://orders_app:Orchid-Lantern-42!@db.staging.example:5432/orders";
    for line in [
        format!("DATABASE_URL={url}\n"),
        format!("database_url: {url}\n"),
        format!("{{\"databaseUrl\": \"{url}\"}}\n"),
        format!("    environment:\n      - DATABASE_URL={url}\n"),
        format!("url: {url}\n"),
        format!("connect with {url} please\n"),
    ] {
        let text = format!("{line}psql login with Orchid-Lantern-42! failed\n");
        let out = scan_at(&text, Sensitivity::Balanced);
        let pw = text.find("Orchid-Lantern-42!").unwrap();
        assert!(
            out.findings
                .iter()
                .any(|f| f.start == pw && f.end == pw + "Orchid-Lantern-42!".len()),
            "{text:?}: {:?}",
            out.findings
        );
        for visible in ["postgres://orders_app:", "@db.staging.example:5432/orders"] {
            assert!(out.output.contains(visible), "{text:?}: {}", out.output);
        }
        assert!(!out.output.contains("Orchid"), "{}", out.output);
        let placeholders: Vec<&str> = out
            .findings
            .iter()
            .map(|f| f.placeholder.as_str())
            .collect();
        assert_eq!(placeholders.len(), 2, "{text:?}: {:?}", out.findings);
        assert_eq!(placeholders[0], placeholders[1], "{text:?}");
    }
}

/// An unterminated quote ending in a backslash does not swallow the next line.
#[test]
fn quoted_value_stops_at_escaped_newline() {
    assert_redacted(
        "\"password\": \"\\\nDB_PASSWORD=Hunter2-Fake-9x\n",
        "Hunter2-Fake-9x",
    );
    assert_redacted(
        "msg = 'see \\\r\nDB_PASSWORD=Hunter2-Fake-9x\r\n",
        "Hunter2-Fake-9x",
    );
}

/// Every secret pair on a logfmt line is found, and repeated values share a number:
/// `#1`, `#2`, `#1`. Non-secret fields stay.
#[test]
fn logfmt_pairs_each_redacted_and_numbered() {
    let text = "primary_password=Tulip-Harbor-91 backup_password=Cedar-Quill-37 \
                retry_password=Tulip-Harbor-91\n";
    let out = scan_at(text, Sensitivity::Balanced);
    assert_eq!(
        out.output,
        "primary_password=[REDACTED: Password #1] backup_password=[REDACTED: Password #2] \
         retry_password=[REDACTED: Password #1]\n"
    );
    let text = "ts=2026-11-06T10:15:02Z level=info user=alice password=Tulip-Harbor-91 \
                token=Zq81mXv0pLr3TfK9wQe2Lm api_key=Cedar-Quill-37 status=200 path=/login\n";
    let out = scan_at(text, Sensitivity::Balanced);
    for v in [
        "Tulip-Harbor-91",
        "Zq81mXv0pLr3TfK9wQe2Lm",
        "Cedar-Quill-37",
    ] {
        assert!(!out.output.contains(v), "{v}: {}", out.output);
    }
    for v in [
        "level=info",
        "user=alice",
        "status=200",
        "path=/login",
        "ts=2026-11-06T10:15:02Z",
    ] {
        assert!(out.output.contains(v), "{v}: {}", out.output);
    }
}

/// A key with a longer description before its connector, in several languages; a `word:`
/// ends the description; Persian puts the verb after the value.
#[test]
fn prose_key_with_long_description() {
    for (text, secret) in [
        (
            "Le mot de passe de la base de donnees est Winter2024! pour l'utilisateur demo.",
            "Winter2024!",
        ),
        (
            "La contraseña de la base de datos es Winter2024! para el usuario demo.",
            "Winter2024!",
        ),
        ("Das Passwort fuer das Backup: S3cr3t!Pass", "S3cr3t!Pass"),
        (
            "رمز عبور Kx9!mPq2vL است و لطفا آن را به اشتراک نگذارید.",
            "Kx9!mPq2vL",
        ),
        (
            "کلمه عبور پایگاه داده Zebra!Lamp(42) می\u{200C}باشد.",
            "Zebra!Lamp(42)",
        ),
        ("گذرواژه Winter2024! برای مدیر سیستم است.", "Winter2024!"),
    ] {
        assert_redacted(text, secret);
    }
    // A description that names something else is still no key.
    assert_clean("the password for the account was changed: yesterday at noon");
}

/// "dragonfly" is a word, not an adverb ending in -ly.
#[test]
fn prose_plain_word_ending_in_fly() {
    let out = scan_at("the wiki password is dragonfly", Sensitivity::Paranoid);
    assert_eq!(out.output, "the wiki password is [REDACTED: Password]");
}

/// Password arguments of login commands, including values that end in a bracket.
#[test]
fn command_password_arguments() {
    for (text, secret) in [
        (
            "tool login -u alice -p Hunter2-Fake-9x\n",
            "Hunter2-Fake-9x",
        ),
        (
            "docker login registry.example.com -u ci-bot -p Zebra!Lamp(42)\n",
            "Zebra!Lamp(42)",
        ),
        (
            "net use Z: \\\\10.0.4.40\\share /user:EXAMPLE\\bob Zebra!Lamp(42)\n",
            "Zebra!Lamp(42)",
        ),
        (
            "net use Z: \\\\fs\\share Zebra!Lamp(42) /user:EXAMPLE\\bob\n",
            "Zebra!Lamp(42)",
        ),
        (
            "htpasswd -b /etc/nginx/.htpasswd alice S3cr3t!Pass\n",
            "S3cr3t!Pass",
        ),
        ("htpasswd -nbB alice S3cr3t!Pass\n", "S3cr3t!Pass"),
        (
            "[09:19] bob: the shared mailbox login is mailer / Zebra!Lamp(42)\n",
            "Zebra!Lamp(42)",
        ),
    ] {
        assert_redacted(text, secret);
    }
    assert_clean("ssh -p 2222 alice@example.com\n");
    assert_clean("htpasswd -c /etc/nginx/.htpasswd alice\n");
    assert_clean("the login is broken / see the ticket\n");
    assert_redacted(
        "The database login is demo / Password: Hunter2-Fake-9x and the console\n",
        "Hunter2-Fake-9x",
    );
}

/// The default of a Terraform variable named like a secret.
#[test]
fn terraform_variable_default() {
    assert_only(
        "variable \"admin_password\" {\n  type    = string\n  default = \"Winter2024!\"\n  sensitive = true\n}\n",
        "Winter2024!",
    );
    assert_clean("variable \"region\" {\n  default = \"eu-west-1\"\n}\n");
    assert_clean("variable \"db_password\" {\n  type    = string\n  default = \"\"\n}\n");
}

/// PowerShell cmdlets and Docker credential store names are not secrets.
#[test]
fn cmdlets_and_credential_stores_are_not_values() {
    assert_clean("$cred = New-Object System.Management.Automation.PSCredential($user, $secure)\n");
    assert_clean("$token = Get-Content token.txt\n");
    assert_clean(
        "{\n  \"credsStore\": \"desktop\",\n  \"credHelpers\": { \"gcr.io\": \"gcloud\" }\n}\n",
    );
    assert_redacted("key_store_password = Hunter2-Fake-9x\n", "Hunter2-Fake-9x");
}

/// Escaped quotes and backslashes in a JSON password are part of it, not path separators.
#[test]
fn json_escaped_password_is_not_a_path() {
    assert_redacted(
        "{\n  \"password\": \"Qu0te\\\"Back\\\\slash!\"\n}\n",
        "Qu0te\\\"Back\\\\slash!",
    );
    assert_clean("{\n  \"password_file\": \"C:\\\\Users\\\\bob\\\\secrets\\\\db.txt\"\n}\n");
}

/// A key after `(`, `,` or `{` is in an argument list, a dump or a list in prose: a literal
/// value there is redacted, exactly; an identifier, member path or call stays code.
#[test]
fn key_after_comma_or_paren_takes_a_literal() {
    assert_redacted(
        "Username: admin, Password: Tulip-Harbor-91\n",
        "Tulip-Harbor-91",
    );
    assert_redacted("root@10.0.0.5 (pw: Tulip-Harbor-91)\n", "Tulip-Harbor-91");
    assert_redacted(
        "User(id=42, password=Tulip-Harbor-91, active=true)\n",
        "Tulip-Harbor-91",
    );
    assert_redacted(
        "helm install api ./chart --set auth.username=app,auth.password=Tulip-Harbor-91,auth.database=shop\n",
        "Tulip-Harbor-91",
    );
    assert_redacted("app.password=Tulip-Harbor-91\n", "Tulip-Harbor-91");
    assert_clean("client = Client(url, username=user, password=pw, timeout=30)\n");
    assert_clean("createUser({ name: name, password: hashed, role: \"admin\" })\n");
    assert_clean("    self.password = password\n");
    assert_clean("config.password = os.environ[\"DB_PASSWORD\"]\n");
    assert_clean("ctx.secret = ctx.secret || defaultSecret;\n");
}

/// Markdown emphasis and inline tags around a key are markup; a key alone in a table cell
/// takes the next cell; a secret-named column takes the cells under it.
#[test]
fn markup_and_table_cells_around_a_key() {
    for text in [
        "**Password:** Tulip-Harbor-91\n",
        "*Password*: Tulip-Harbor-91\n",
        "<strong>Password:</strong> Tulip-Harbor-91\n",
        "<b>Password</b>: Tulip-Harbor-91<br>\n",
        "<tr><td>Password</td><td>Tulip-Harbor-91</td></tr>\n",
        "<dt>Password</dt>\n  <dd>Tulip-Harbor-91</dd>\n",
        "| Password | Tulip-Harbor-91 |\n",
        "| username | password |\n|---|---|\n| admin | Tulip-Harbor-91 |\n",
        "username,password,role\nadmin,Tulip-Harbor-91,owner\n",
    ] {
        assert_redacted(text, "Tulip-Harbor-91");
    }
    assert_clean("**Password:** the one you chose when you signed up.\n");
    assert_clean("| password | string | yes | The account password |\n");
    assert_clean("| Name | Description |\n|---|---|\n| password | The account password |\n");
    assert_clean("<tr><td>password</td><td>The password of the account</td></tr>\n");
}

/// A secret name in a `name`/`key` field takes the sibling `value` field of its object or
/// list item; a secret-named object takes its nested `value`.
#[test]
fn name_value_siblings_and_nested_values() {
    for text in [
        "env:\n  - name: DB_PASSWORD\n    value: Tulip-Harbor-91\n",
        "env:\n  - name: \"DB_PASSWORD\"\n    value: \"Tulip-Harbor-91\"\n",
        "[{\"name\": \"DB_PASSWORD\", \"value\": \"Tulip-Harbor-91\"}]\n",
        "{\n  \"name\": \"SMTP_PASSWORD\",\n  \"slotSetting\": false,\n  \"value\": \"Tulip-Harbor-91\"\n}\n",
        "{ \"key\": \"X-Api-Key\", \"value\": \"Tulip-Harbor-91\", \"type\": \"text\" }\n",
        "\"db_password\": {\n  \"value\": \"Tulip-Harbor-91\",\n  \"sensitive\": true\n}\n",
        "<setting name=\"SmtpPassword\" serializeAs=\"String\">\n  <value>Tulip-Harbor-91</value>\n</setting>\n",
        "<AdministratorPassword>\n  <Value>Tulip-Harbor-91</Value>\n</AdministratorPassword>\n",
    ] {
        assert_redacted(text, "Tulip-Harbor-91");
    }
    assert_clean("env:\n  - name: APP_ENV\n    value: \"production\"\n");
    assert_clean(
        "env:\n  - name: DB_PASSWORD\n    valueFrom:\n      secretKeyRef:\n        name: db\n        key: password\n  - name: MODE\n    value: Lantern-Fox-12\n",
    );
    assert_clean("[{\"name\": \"PORT\", \"value\": \"8080\"}, {\"name\": \"DB_PASSWORD\"}]\n");
}

/// A key whose words classify is a name, however random its letters and digits look; a long
/// dotted or underscored name is still one token.
#[test]
fn long_and_digit_key_names_are_keys() {
    for key in [
        "OAUTH2_PROXY_CLIENT_SECRET",
        "NEO4J_PASSWD",
        "PG15_PASSWORD",
        "AZURE_AD_B2C_CLIENT_SECRET",
        "spring.security.oauth2.client.registration.azure-active-directory.client-secret",
        "SPRING_SECURITY_OAUTH2_CLIENT_REGISTRATION_AZURE_ACTIVE_DIRECTORY_CLIENT_SECRET",
    ] {
        assert_redacted(&format!("{key}=Tulip-Harbor-91\n"), "Tulip-Harbor-91");
    }
    assert_redacted(
        "oauth2ProxyClientSecret: Tulip-Harbor-91\n",
        "Tulip-Harbor-91",
    );
    // A one-letter key word (`b` of `B2`) found in a random value is no echo.
    assert_redacted(
        "B2_APPLICATION_KEY=K001aB3dE5fG7hI9jK1lM3nOpQ5\n",
        "K001aB3dE5fG7hI9jK1lM3nOpQ5",
    );
    // The key and its `=` before a placeholder are not base64 with padding.
    let out = scan_at(
        "AZURE_AD_B2C_CLIENT_SECRET=Tulip-Harbor-91\n",
        Sensitivity::Balanced,
    );
    assert!(
        out.output
            .starts_with("AZURE_AD_B2C_CLIENT_SECRET=[REDACTED"),
        "{}",
        out.output
    );
    assert_clean("OAUTH2_PROXY_COOKIE_SECRET_LENGTH=32\n");
}

/// Code words (`temp`, `new`, `user`, `data`) make a variable only in code or without digits
/// among the words; on a config line or in a JSON string `Temp1234` is a password.
#[test]
fn word_like_passwords_outside_code() {
    for (text, v) in [
        (
            "{\"username\": \"jdoe\", \"password\": \"Temp1234\"}\n",
            "Temp1234",
        ),
        ("SMTP_PASSWORD=Temp1234\n", "Temp1234"),
        ("SMTP_PASSWORD=new_pass\n", "new_pass"),
        ("  password: user2024\n", "user2024"),
        ("spring.datasource.password=Temp1234\n", "Temp1234"),
        ("Login: jdoe / Password: Temp1234\n", "Temp1234"),
    ] {
        assert_redacted(text, v);
    }
    assert_clean("    password = new_password\n");
    assert_clean("{\"password\": \"userPassword\"}\n");
    assert_clean("login(username=user1, password=pass1)\n");
}

/// Prose connectors: `is:`, `set/reset/changed … to`, `will/should be`, `-`, a qualifier with
/// a colon, the value before `is the password`, and the German, Italian, French and Persian
/// forms.
#[test]
fn prose_connectors_name_the_value() {
    for text in [
        "Your new password is: Tulip-Harbor-91\n",
        "Your password has been reset to: Tulip-Harbor-91\n",
        "Your password was reset to Tulip-Harbor-91\n",
        "I changed the password to Tulip-Harbor-91 yesterday.\n",
        "The password will be Tulip-Harbor-91\n",
        "Password (prod): Tulip-Harbor-91\n",
        "Password - Tulip-Harbor-91\n",
        "\"Tulip-Harbor-91\" is the password\n",
        "Tulip-Harbor-91 is the new admin password\n",
        "Ich habe das Passwort auf Tulip-Harbor-91 geändert.\n",
        "La password è Tulip-Harbor-91\n",
        "le mot de passe c'est Tulip-Harbor-91\n",
        "پسورد سرور: Tulip-Harbor-91\n",
        "رمز عبور را به Tulip-Harbor-91 تغییر دادم\n",
    ] {
        assert_redacted(text, "Tulip-Harbor-91");
    }
    for text in [
        "The password will be emailed to you.\n",
        "Your password has been reset to the default policy.\n",
        "This is the password field.\n",
        "the password for the account was changed: yesterday at noon\n",
    ] {
        let out = scan_at(text, Sensitivity::Balanced);
        assert!(out.findings.is_empty(), "{text:?}: {:?}", out.findings);
    }
}

/// A secret passed to a setting call, a setter, a fallback, a plan diff, a PHP dump or a
/// SQL insert is redacted; code around it is not.
#[test]
fn call_arguments_fallbacks_and_dumps() {
    for text in [
        "define( 'DB_PASSWORD', 'Tulip-Harbor-91' );\n",
        "os.environ.setdefault(\"DB_PASSWORD\", \"Tulip-Harbor-91\")\n",
        "password = os.getenv(\"DB_PASSWORD\", \"Tulip-Harbor-91\")\n",
        "dataSource.setPassword(\"Tulip-Harbor-91\");\n",
        "const password = process.env.DB_PASSWORD || \"Tulip-Harbor-91\";\n",
        "  ~ password = \"Old-Pass-1x\" -> \"Tulip-Harbor-91\"\n",
        "Array\n(\n    [password] => Tulip-Harbor-91\n)\n",
        "  [\"password\"]=>\n  string(15) \"Tulip-Harbor-91\"\n",
        "INSERT INTO users (username, password, role) VALUES ('admin', 'Tulip-Harbor-91', 'owner');\n",
    ] {
        assert_redacted(text, "Tulip-Harbor-91");
    }
    assert_clean("define('WP_DEBUG', false);\ndefine('DB_PASSWORD', getenv('DB_PASSWORD'));\n");
    assert_clean("echo __( 'Password', 'woocommerce' );\n");
    assert_clean("self.secret = secret.encode(\"utf-8\")\n");
    assert_clean("INSERT INTO users (email, password) VALUES (?, ?);\n");
}

/// `user / password` after a credential key, a `user/password` key or a login word: the part
/// after the slash is the password.
#[test]
fn user_slash_password_pairs() {
    for text in [
        "Credentials: admin / Tulip-Harbor-91\n",
        "Zugangsdaten: admin / Tulip-Harbor-91\n",
        "Identifiants : admin / Tulip-Harbor-91\n",
        "Login: admin / Tulip-Harbor-91\n",
        "User/Pass: admin / Tulip-Harbor-91\n",
        "u: admin p: Tulip-Harbor-91\n",
    ] {
        assert_redacted(text, "Tulip-Harbor-91");
    }
    assert_clean("Credentials: see the vault / ask the team lead\n");
    assert_clean("Credentials: AWS / GCP\n");
    assert_clean("Login: Google / GitHub / email\n");
    assert_clean("u: 3 p: 4\n");
}

/// Password arguments of database, remote-access, archive and account commands, a secret
/// piped into `sudo -S`, `--password-stdin` or `chpasswd`, and config directives that take
/// the password after a blank.
#[test]
fn tool_password_arguments() {
    for text in [
        "sqlcmd -S localhost -U sa -P 'Tulip-Harbor-91' -Q \"SELECT 1\"\n",
        "redis-cli -h cache.internal -p 6379 -a Tulip-Harbor-91 ping\n",
        "mongosh \"mongodb://db.internal:27017/shop\" -u root -p Tulip-Harbor-91\n",
        "sshpass -pTulip-Harbor-91 ssh root@10.0.0.9\n",
        "echo \"Tulip-Harbor-91\" | sudo -S systemctl restart nginx\n",
        "echo \"Tulip-Harbor-91\" | docker login -u ci --password-stdin registry.example.com\n",
        "echo 'deploy:Tulip-Harbor-91' | chpasswd\n",
        "mysql -h db.internal -u root -p Tulip-Harbor-91\n",
        "xfreerdp /v:10.0.0.9 /u:Administrator /p:Tulip-Harbor-91 /cert:ignore\n",
        "schtasks /create /tn backup /tr c:\\b.cmd /ru CORP\\svc /rp Tulip-Harbor-91\n",
        "net user svc_backup Tulip-Harbor-91 /add\n",
        "rabbitmqctl add_user worker Tulip-Harbor-91\n",
        "useradd -m -p Tulip-Harbor-91 deploy\n",
        "smbclient //fileserver/share -U jdoe%Tulip-Harbor-91\n",
        "lftp -u deploy,Tulip-Harbor-91 sftp://files.example.com\n",
        "http -a admin:Tulip-Harbor-91 GET https://api.example.com/v1/users\n",
        "expdp scott/Tulip-Harbor-91@ORCL schemas=SCOTT dumpfile=scott.dmp\n",
        "db2 connect to SAMPLE user db2inst1 using Tulip-Harbor-91\n",
        "openssl enc -aes-256-cbc -salt -in a.txt -out a.enc -k Tulip-Harbor-91\n",
        "unrar x -pTulip-Harbor-91 archive.rar\n",
        "ssh-keygen -t ed25519 -N \"Tulip-Harbor-91\" -f ~/.ssh/id_deploy\n",
        "$sec = ConvertTo-SecureString -AsPlainText \"Tulip-Harbor-91\" -Force\n",
        "Set-LocalUser -Name \"svc\" -Password (ConvertTo-SecureString -AsPlainText \"Tulip-Harbor-91\" -Force)\n",
        "rootpw --plaintext Tulip-Harbor-91\n",
        "bindpw Tulip-Harbor-91\n",
        "enable secret 0 Tulip-Harbor-91\n",
        "username admin privilege 15 secret 0 Tulip-Harbor-91\n",
        "snmp-server community Tulip-Harbor-91 RO\n",
    ] {
        assert_redacted(text, "Tulip-Harbor-91");
    }
    assert_redacted(
        "vault login s.Zq81mXv0pLr3TfK9wQe2Lm4x\n",
        "s.Zq81mXv0pLr3TfK9wQe2Lm4x",
    );
    for text in [
        "  -p, --password <password>   Password to use when connecting to the server\n",
        "mysql -u root -p shop\n",
        "redis-cli -h cache.internal -p 6379 info\n",
        "echo \"$SUDO_PASSWORD\" | sudo -S apt-get update\n",
        "sudo -S systemctl restart nginx\n",
        "echo \"hello world\" | sudo tee /etc/motd\n",
        "net user svc_backup * /add\n",
        "ssh-keygen -t ed25519 -N \"\" -f ~/.ssh/id_deploy\n",
        "vault login -method=oidc\n",
        "unrar x -p- archive.rar\n",
    ] {
        assert_clean(text);
    }
}

/// A webhook URL key: the random path segment is the secret; a plain hook path is not.
#[test]
fn webhook_url_keys_take_the_random_segment() {
    assert_redacted(
        "WEBHOOK_URL=https://hooks.example.com/in/Zq81mXv0pLr3TfK9wQe2Lm4x\n",
        "Zq81mXv0pLr3TfK9wQe2Lm4x",
    );
    assert_redacted(
        "alert_webhook_url: https://alerts.example.com/hook/p4Rt9Xw2Zq7Lm3Kv8Bn6?v=2\n",
        "p4Rt9Xw2Zq7Lm3Kv8Bn6",
    );
    assert_clean("WEBHOOK_URL=https://hooks.example.com/in/team-alerts\n");
    assert_clean("WEBHOOK_URL=https://example.com/webhooks/github\n");
}

/// A negative word before the head says which secret it is (`APPLE_ID_PASSWORD`,
/// `ZIP_FILE_PASSWORD`, `INPUT_PASSWORD`); after the head, or right before it as a modifier
/// (`max_tokens`, `public_key`, `has_password`), it still voids the key.
#[test]
fn negative_words_before_the_head() {
    for text in [
        "APPLE_ID_PASSWORD=Tulip-Harbor-91\n",
        "ZIP_FILE_PASSWORD=Tulip-Harbor-91\n",
        "EMAIL_PROVIDER_PASSWORD=Tulip-Harbor-91\n",
        "INPUT_PASSWORD=Tulip-Harbor-91\n",
        "URL_SIGNING_SECRET=Tulip-Harbor-91\n",
        "PUBLIC_SITE_ADMIN_PASSWORD=Tulip-Harbor-91\n",
        "one_time_password: Tulip-Harbor-91\n",
        "clearTextPassword: Tulip-Harbor-91\n",
        "password_text: Tulip-Harbor-91\n",
        "export TF_VAR_db_password=\"Tulip-Harbor-91\"\n",
        "env.DB_PASSWORD = 'Tulip-Harbor-91'\n",
    ] {
        assert_redacted(text, "Tulip-Harbor-91");
    }
    assert_redacted(
        "VAULT_SECRET_ID=84896a0c-1347-aa90-a4f6-aca8b7558780\n",
        "84896a0c-1347-aa90-a4f6-aca8b7558780",
    );
    for text in [
        "MAX_TOKENS=4096\n",
        "PUBLIC_KEY=ssh-ed25519.pub\n",
        "has_password: true\n",
        "PASSWORD_MIN_LENGTH=12\n",
        "password_text: Enter your password\n",
        "FILE_ID=8f2c1a\n",
    ] {
        assert_clean(text);
    }
}

/// Key words the lexicon lacked: glued and short password names, PSK, authtok, passkey, and
/// German, French and Persian key words.
#[test]
fn lexicon_gap_key_words() {
    for text in [
        "DBPASS=Tulip-Harbor-91\n",
        "$dbpass = 'Tulip-Harbor-91';\n",
        "userpass: Tulip-Harbor-91\n",
        "rootpw: Tulip-Harbor-91\n",
        "DBPWD=Tulip-Harbor-91\n",
        "psw=Tulip-Harbor-91\n",
        "pswd: Tulip-Harbor-91\n",
        "passwrd: Tulip-Harbor-91\n",
        "SECRETKEY=Tulip-Harbor-91\n",
        "passkey: Tulip-Harbor-91\n",
        "VPN_IPSEC_PSK=Tulip-Harbor-91\n",
        "    psk=\"Tulip-Harbor-91\"\n",
        "ldap_default_authtok = Tulip-Harbor-91\n",
        "WLAN-Schlüssel: Tulip-Harbor-91\n",
        "Zugangscode: Tulip-Harbor-91\n",
    ] {
        assert_redacted(text, "Tulip-Harbor-91");
    }
    let key = "k3j9x7q2m5n8p4w1v6b2c4d8f1g3h5z7";
    for text in [
        format!("API-Schlüssel: {key}\n"),
        format!("Geheimer Schlüssel: {key}\n"),
        format!("Clé API : {key}\n"),
        format!("Jeton d'accès : {key}\n"),
        format!("کلید API: {key}\n"),
    ] {
        assert_redacted(&text, key);
    }
    assert_redacted(
        "Authorization: 9f8e7d6c5b4a39281706f5e4d3c2b1a0\n",
        "9f8e7d6c5b4a39281706f5e4d3c2b1a0",
    );
    for text in [
        "Passkey support is coming to the login page next quarter.\n",
        "Authorization: required\n",
        "vault auth enable userpass\n",
        "Der Schlüssel liegt im Büro.\n",
    ] {
        assert_clean(text);
    }
}

/// Digits, short values and letterless values: 6+ digits for a password key on a config line
/// or in a JSON string, PIN-like codes (passcode, OTP, CVV, Persian digits), five-character
/// YAML and INI values with digits, symbols with digits, and a word in a logfmt field.
#[test]
fn digit_short_and_letterless_passwords() {
    for (text, v) in [
        ("DB_PASSWORD=12345678\n", "12345678"),
        ("POSTGRES_PASSWORD=482917\n", "482917"),
        ("wifi:\n  password: \"4829175036\"\n", "4829175036"),
        (
            "{\"username\": \"admin\", \"password\": \"20240815\"}\n",
            "20240815",
        ),
        ("door passcode: 482917\n", "482917"),
        ("رمز کارت: ۴۸۲۱\n", "۴۸۲۱"),
        ("OTP: 482917\n", "482917"),
        ("cvv: 847\n", "847"),
        ("db:\n  password: abc12\n", "abc12"),
        ("[database]\npassword = kiwi9\n", "kiwi9"),
        ("DB_PASSWORD=1234!5678#\n", "1234!5678#"),
        (
            "level=info user=bob password=sunshine attempt=2\n",
            "sunshine",
        ),
    ] {
        assert_redacted(text, v);
    }
    for text in [
        "PORT=5432\n",
        "TIMEOUT=30000000\n",
        "FILE_ID=12345678\n",
        "password: 2024\n",
        "otp_length: 6\n",
        "level=info user=bob password=redacted attempt=2\n",
        "level=info user=bob password=present attempt=2\n",
    ] {
        assert_clean(text);
    }
}

/// After `,` or `(` an identifier-shaped value is code, unless a strong password key holds a
/// value with digits mixed into its letters (`pw: Temp1234`); `password=pw_value` and
/// `password=password` stay code.
#[test]
fn mixed_case_digit_password_after_comma() {
    assert_redacted(
        "The test account (user: qa1, pw: Temp1234) is locked.\n",
        "Temp1234",
    );
    assert_redacted("(user: qa1, pw: Temp1234)\n", "Temp1234");
    assert_redacted("connect(host=db1, password=Kiwi2024)\n", "Kiwi2024");
    for text in [
        "foo(a, b, password=pw_value)\n",
        "f(password=password)\n",
        "login(username=user1, password=pass1)\n",
        "client = Client(user, password=userPassword)\n",
    ] {
        assert_clean(text);
    }
}

/// Help text puts metavariables where the password goes: `[DATABASE]`, `PASSWORD`,
/// `<password>`, `{password}`, `***`. A real password in the same place is redacted.
#[test]
fn help_text_metavariables_are_not_values() {
    for text in [
        "Usage: mysql -u USER -p [DATABASE]\n",
        "Usage: mysql -u USER -pPASSWORD [DATABASE]\n",
        "unrar x -pPASSWORD archive.rar\n",
        "7z a -pSECRET out.7z folder\n",
        "ssh-keygen -p -P OLD_PASSPHRASE -N NEW_PASSPHRASE -f keyfile\n",
        "snmp-server community STRING RO\n",
        "openssl enc -aes-256-cbc -k PASSPHRASE -in FILE -out FILE.enc\n",
        "7z x -p{password} archive.7z\n",
        "redis-cli -a *** ping\n",
        "DB_PASSWORD=[DATABASE]\n",
    ] {
        assert_clean(text);
    }
    assert_redacted("mysql -u USER -p Hunter2-Fake-9x shop\n", "Hunter2-Fake-9x");
    assert_redacted("7z a -pTulip-Harbor-91 out.7z folder\n", "Tulip-Harbor-91");
    assert_redacted("DB_PASSWORD=[Tulip-Harbor-91]\n", "[Tulip-Harbor-91]");
}

/// A config value keeps characters that look like code: glued operators, `::`, calls with
/// a number, YAML tag or alias marks before a password, rules inside one word, the default
/// of a `${VAR:-v}` reference.
#[test]
fn config_values_that_look_like_code() {
    for (text, secret) in [
        ("DB_PASSWORD=kT9#mQ2::xL7pW4z\n", "kT9#mQ2::xL7pW4z"),
        ("DB_PASSWORD=aB3&&dE5fG7hJ9\n", "aB3&&dE5fG7hJ9"),
        ("DB_PASSWORD=Why??Not123x\n", "Why??Not123x"),
        ("API_SECRET=mN5=>bV7cX9zL1\n", "mN5=>bV7cX9zL1"),
        ("API_SECRET=qW3!(eR5tY7uI9\n", "qW3!(eR5tY7uI9"),
        ("API_SECRET=hG6).fD4sA2pO8\n", "hG6).fD4sA2pO8"),
        ("DB_PASSWORD=Summer(2024)\n", "Summer(2024)"),
        ("DB_PASSWORD=Hunter[9]Zed[2]\n", "Hunter[9]Zed[2]"),
        ("API_KEY=alpha1:bravo2:charlie3\n", "alpha1:bravo2:charlie3"),
        ("DB_PASSWORD=lambda2024!x\n", "lambda2024!x"),
        ("DB_PASSWORD=Abc<9xK>\n", "Abc<9xK>"),
        ("DB_PASSWORD=blue---sky42\n", "blue---sky42"),
        ("DB_PASSWORD=Xy***9kLm2\n", "Xy***9kLm2"),
        (
            "POSTGRES_PASSWORD=${POSTGRES_PASSWORD:-hunter2prodX}\n",
            "hunter2prodX",
        ),
        (
            "    environment:\n      POSTGRES_PASSWORD: ${PG_PASS:-Sup3rDevPw}\n",
            "Sup3rDevPw",
        ),
        ("password: !Summer2024\n", "!Summer2024"),
        ("Password: *Star2024x\n", "*Star2024x"),
        ("passphrase: \"my dog Rex 2019\"\n", "my dog Rex 2019"),
        ("password: \"Hello World 123!\"\n", "Hello World 123!"),
        ("{\"password\": \"hunter2prod \"}\n", "hunter2prod"),
    ] {
        assert_redacted(text, secret);
    }
    for text in [
        "  password: process.env.DB_PASSWORD || undefined,\n",
        "password: this.pw||null\n",
        "secret: opts.secret??''\n",
        "password = sys.argv[1]\n",
        "token = std::env::var(\"TOKEN\")\n",
        "password: *default_password\n",
        "token: !Ref ApiToken\n",
        "DB_PASSWORD=${DB_PASSWORD:-}\n",
        "token: Option<String>,\n",
        "passphrase: \"at least four words\"\n",
        "password: \"Must be at least 8 characters\"\n",
    ] {
        assert_clean(text);
    }
}

/// A JSON document escaped inside a JSON string: `{\"password\":\"v\"}`; and a random value
/// after a `\n` escape.
#[test]
fn escaped_json_keys_and_values() {
    assert_redacted(
        r#"{"level":"debug","body":"{\"username\":\"admin\",\"password\":\"Tr0ub4dor&3\"}"}"#,
        "Tr0ub4dor&3",
    );
    assert_redacted(
        r#"{"msg":"payload {\"api_key\": \"Zq81mXv0pLr3TfK9wQe2Lm4x\", \"retry\": 3}"}"#,
        "Zq81mXv0pLr3TfK9wQe2Lm4x",
    );
    assert_redacted(
        r#"{"output": "Your API key:\nZq81mXv0pLr3TfK9wQe2Lm4x\nKeep it safe"}"#,
        "Zq81mXv0pLr3TfK9wQe2Lm4x",
    );
    let pat = Gen::named("escaped").github("ghp_");
    assert_redacted(&format!("echo -e \"\\x1b[32m{pat}\\x1b[0m\"\n"), &pat);
    // The decode layer reads the `\u001b` string as a whole: the token goes, with its codes.
    let text = format!("{{\"msg\": \"\\u001b[31m{pat}\\u001b[0m\"}}\n");
    assert!(!scan_at(&text, Sensitivity::Balanced).output.contains(&pat));
    for text in [
        r#"{"level":"debug","body":"{\"username\":\"admin\",\"password\":\"<redacted>\"}"}"#,
        r#"{"level":"debug","body":"{\"password\":null,\"remember\":true}"}"#,
        r#"{"log":"Compiling serde v1.0.210\n   Compiling tokio v1.40.0\n"}"#,
    ] {
        assert_clean(text);
    }
}
