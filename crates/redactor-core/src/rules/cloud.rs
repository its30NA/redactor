//! Cloud providers. Some secrets here have no prefix of their own (AWS secret keys, Azure
//! DevOps PATs, Cloudflare tokens); those rules anchor on the key name in front of them and
//! carry a lower confidence than prefixed tokens.

use super::{check, Bound, Family, Hit, Rule, Verdict};

fn r(
    id: &'static str,
    label: &'static str,
    shape: &'static str,
    anchors: &'static [&'static str],
    re: &'static str,
    conf: f32,
) -> Rule {
    Rule::new(Family::Cloud, id, label, shape, anchors, re, conf)
}

/// AWS documentation examples end in `EXAMPLE`; they are not secrets.
fn not_aws_example(h: &Hit) -> Verdict {
    if h.secret().ends_with("EXAMPLE") {
        Verdict::Reject
    } else {
        Verdict::Keep
    }
}

/// `sig=` is an Azure SAS signature only next to the other SAS parameters.
fn azure_sas(h: &Hit) -> Verdict {
    if check::after_query_sep(h) == Verdict::Reject {
        return Verdict::Reject;
    }
    let url_start = h.before(600);
    let url_start = url_start
        .rfind(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        .map_or(url_start, |i| &url_start[i..]);
    if ["sv=", "se=", "sp=", "srt=", "sr="]
        .iter()
        .any(|p| url_start.contains(p))
    {
        Verdict::Keep
    } else {
        Verdict::Reject
    }
}

pub(super) fn rules() -> Vec<Rule> {
    vec![
        r(
            "aws_access_key_id",
            "AWS Access Key ID",
            "AKIA/ASIA/AGPA/AIDA/AROA/ANPA/ABIA/ACCA + 16 [A-Z2-7]",
            &["AKIA", "ASIA", "AGPA", "AIDA", "AROA", "ANPA", "ABIA", "ACCA"],
            r"(?:AKIA|ASIA|AGPA|AIDA|AROA|ANPA|ABIA|ACCA)[A-Z2-7]{16}",
            0.95,
        )
        .check(not_aws_example),
        r(
            "aws_secret_access_key",
            "AWS Secret Access Key",
            "aws_secret_access_key = 40 base64 chars",
            &["aws_secret_access_key", "aws_secret_key", "secretaccesskey", "secret_access_key"],
            r#"(?i:aws_secret_access_key|aws_secret_key|secretaccesskey|secret_access_key)["']?\s*(?:=>|:=|[:=])\s*["']?(?P<s>[A-Za-z0-9/+]{40})"#,
            0.9,
        )
        .ci()
        .back(4)
        .fwd(120)
        .bounds(Bound::None, Bound::Base64),
        r(
            "aws_session_token",
            "AWS Session Token",
            "aws_session_token = 100+ base64 chars",
            &["aws_session_token", "aws_security_token", "sessiontoken"],
            r#"(?i:aws_session_token|aws_security_token|sessiontoken)["']?\s*(?:=>|:=|[:=])\s*["']?(?P<s>[A-Za-z0-9/+=]{100,})"#,
            0.9,
        )
        .ci()
        .fwd(4096)
        .bounds(Bound::None, Bound::Base64),
        r(
            "aws_presigned_signature",
            "AWS Signature",
            "X-Amz-Signature=<64 hex>",
            &["x-amz-signature="],
            r"(?i:x-amz-signature)=(?P<s>[0-9a-f]{64,})",
            0.95,
        )
        .ci()
        .fwd(100)
        .bounds(Bound::None, Bound::Word)
        .check(check::after_query_sep),
        r(
            "aws_presigned_credential",
            "AWS Credential Scope",
            "X-Amz-Credential=<key id>/<scope>",
            &["x-amz-credential="],
            r#"(?i:x-amz-credential)=(?P<s>[^&\s"'<>]{16,})"#,
            0.8,
        )
        .ci()
        .fwd(240)
        .bounds(Bound::None, Bound::None)
        .check(check::after_query_sep),
        r(
            "aws_presigned_security_token",
            "AWS Session Token",
            "X-Amz-Security-Token=<token>",
            &["x-amz-security-token="],
            r#"(?i:x-amz-security-token)=(?P<s>[^&\s"'<>]{40,})"#,
            0.95,
        )
        .ci()
        .fwd(4096)
        .bounds(Bound::None, Bound::None)
        .check(check::after_query_sep),
        r(
            "google_api_key",
            "Google API Key",
            "AIza + 35 [A-Za-z0-9_-]",
            &["AIza"],
            r"AIza[0-9A-Za-z_-]{35}",
            0.95,
        ),
        r(
            "google_oauth_access_token",
            "Google OAuth Token",
            "ya29. + 30+ [A-Za-z0-9_-]",
            &["ya29."],
            r"ya29\.[0-9A-Za-z_-]{30,}",
            0.95,
        )
        .fwd(4096),
        r(
            "google_oauth_refresh_token",
            "Google Refresh Token",
            "1//0 + 30+ [A-Za-z0-9_-]",
            &["1//0"],
            r"1//0[0-9A-Za-z_-]{30,}",
            0.85,
        )
        .fwd(1024)
        .check(check::random),
        r(
            "google_oauth_client_secret",
            "Google OAuth Client Secret",
            "GOCSPX- + 28 [A-Za-z0-9_-]",
            &["GOCSPX-"],
            r"GOCSPX-[A-Za-z0-9_-]{28}",
            0.99,
        ),
        r(
            "gcp_signed_url_signature",
            "Google Cloud Signature",
            "X-Goog-Signature=<256+ hex>",
            &["x-goog-signature="],
            r"(?i:x-goog-signature)=(?P<s>[0-9a-f]{256,})",
            0.95,
        )
        .ci()
        .fwd(2048)
        .bounds(Bound::None, Bound::Word)
        .check(check::after_query_sep),
        r(
            "azure_storage_account_key",
            "Azure Storage Key",
            "AccountKey=<86 base64>==",
            &["accountkey="],
            r"(?i:accountkey)=(?P<s>[A-Za-z0-9+/]{86}==)",
            0.98,
        )
        .ci()
        .fwd(110)
        .bounds(Bound::Word, Bound::Base64),
        r(
            "azure_shared_access_key",
            "Azure Shared Access Key",
            "SharedAccessKey=<40-90 base64>",
            &["sharedaccesskey="],
            r"(?i:sharedaccesskey)=(?P<s>[A-Za-z0-9+/=]{40,})",
            0.97,
        )
        .ci()
        .fwd(110)
        .bounds(Bound::Word, Bound::Base64),
        r(
            "azure_sas_signature",
            "Azure SAS Signature",
            "…sv=…&sig=<url-encoded base64>",
            &["sig="],
            r"sig=(?P<s>[A-Za-z0-9%+/=]{40,})",
            0.95,
        )
        .fwd(130)
        .bounds(Bound::None, Bound::Base64)
        .check(azure_sas),
        r(
            "azure_ad_client_secret",
            "Azure AD Client Secret",
            "3 chars + digit + Q~ + 34 chars",
            &["Q~"],
            r"[A-Za-z0-9_~.]{3}[0-9]Q~[A-Za-z0-9_~.-]{34}",
            0.95,
        )
        .back(4)
        .fwd(40),
        r(
            "azure_devops_pat",
            "Azure DevOps Token",
            "azure_devops_pat = 52 [a-z2-7] or 84 [A-Za-z0-9]",
            &["azure_devops_pat", "azure_devops_token", "azure_devops_ext_pat", "ado_pat", "system_accesstoken"],
            r#"(?i:azure_devops_pat|azure_devops_token|azure_devops_ext_pat|ado_pat|system_accesstoken)["']?\s*(?:=>|:=|[:=])\s*["']?(?P<s>[a-z2-7]{52}|[A-Za-z0-9]{84})"#,
            0.85,
        )
        .ci()
        .fwd(160)
        .bounds(Bound::None, Bound::Word),
        r(
            "azure_devops_pat_v2",
            "Azure DevOps Token",
            "76 [A-Za-z0-9] + AZDO + 4 [A-Za-z0-9]",
            &["AZDO"],
            r"[A-Za-z0-9]{76}AZDO[A-Za-z0-9]{4}",
            0.97,
        )
        .back(76)
        .fwd(8),
        r(
            "azure_openai_key",
            "Azure OpenAI Key",
            "azure_openai_api_key = 32 hex or 84 [A-Za-z0-9]",
            &["azure_openai_api_key", "azure_openai_key", "azureopenai_key"],
            r#"(?i:azure_openai_api_key|azure_openai_key|azureopenai_key)["']?\s*(?:=>|:=|[:=])\s*["']?(?P<s>[a-f0-9]{32}|[A-Za-z0-9]{84})"#,
            0.85,
        )
        .ci()
        .fwd(160)
        .bounds(Bound::None, Bound::Word),
        r(
            "azure_cosmos_key",
            "Azure Cosmos DB Key",
            "cosmos…key = 86 base64 + ==",
            &["cosmos"],
            r#"(?i:cosmos)[A-Za-z_]{0,30}["']?\s*(?:=>|:=|[:=])\s*["']?(?P<s>[A-Za-z0-9+/]{86}==)"#,
            0.9,
        )
        .ci()
        .fwd(160)
        .bounds(Bound::None, Bound::Base64),
        r(
            "alibaba_access_key_id",
            "Alibaba Cloud Access Key ID",
            "LTAI + 12-20 [A-Za-z0-9]",
            &["LTAI"],
            r"LTAI[A-Za-z0-9]{12,20}",
            0.9,
        )
        .check(check::random),
        r(
            "tencent_secret_id",
            "Tencent Cloud Secret ID",
            "AKID + 13-40 [A-Za-z0-9]",
            &["AKID"],
            r"AKID[A-Za-z0-9]{13,40}",
            0.85,
        )
        .check(check::random),
        r(
            "cloudflare_api_token",
            "Cloudflare API Token",
            "cloudflare_api_token = 40 [A-Za-z0-9_-] or 37 hex",
            &[
                "cloudflare_api_token",
                "cloudflare_api_key",
                "cloudflare_token",
                "cf_api_token",
                "cf_api_key",
            ],
            r#"(?i:cloudflare_api_token|cloudflare_api_key|cloudflare_token|cf_api_token|cf_api_key)["']?\s*(?:=>|:=|[:=])\s*["']?(?P<s>[A-Za-z0-9_-]{40}|[a-f0-9]{37})"#,
            0.85,
        )
        .ci()
        .fwd(120)
        .bounds(Bound::None, Bound::Word),
        r(
            "cloudflare_origin_ca_key",
            "Cloudflare Origin CA Key",
            "v1.0- + 24 hex + - + 146 hex",
            &["v1.0-"],
            r"v1\.0-[a-f0-9]{24}-[a-f0-9]{146}",
            0.98,
        ),
    ]
}
