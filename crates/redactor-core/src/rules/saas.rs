//! SaaS APIs, payments, messaging and webhooks.

use super::{check, Bound, Family, Hit, Rule, Verdict};

fn r(
    id: &'static str,
    label: &'static str,
    shape: &'static str,
    anchors: &'static [&'static str],
    re: &'static str,
    conf: f32,
) -> Rule {
    Rule::new(Family::Saas, id, label, shape, anchors, re, conf)
}

/// `key-` + 32 hex is only Mailgun-specific when "mailgun" is nearby.
fn mailgun_context(h: &Hit) -> Verdict {
    let near = h.before(200).to_ascii_lowercase();
    if near.contains("mailgun") || h.line().to_ascii_lowercase().contains("mailgun") {
        Verdict::Conf(0.9, "key- + 32 hex near \"mailgun\"")
    } else {
        Verdict::Keep
    }
}

pub(super) fn rules() -> Vec<Rule> {
    vec![
        r(
            "slack_client_token",
            "Slack Token",
            "xoxc- + 3 number groups + 64+ hex; xoxd- + 40+ cookie characters",
            &["xoxc-", "xoxd-"],
            r"xoxc-[0-9]{10,13}-[0-9]{10,13}-[0-9]{10,13}-[a-f0-9]{64,}|xoxd-[A-Za-z0-9%+/=]{40,}",
            0.98,
        )
        .check(check::random),
        r(
            "hubspot_private_app_token",
            "HubSpot Token",
            "pat- + region + n- + UUID",
            &["pat-na", "pat-eu", "pat-ap"],
            r"pat-(?:na|eu|ap)[0-9]-[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}",
            0.95,
        ),
        r(
            "notion_integration_secret",
            "Notion Token",
            "secret_ + 43 [A-Za-z0-9]",
            &["secret_"],
            r"secret_[A-Za-z0-9]{43}",
            0.85,
        )
        .check(check::random),
        r(
            "notion_token",
            "Notion Token",
            "ntn_ + 40-50 [A-Za-z0-9]",
            &["ntn_"],
            r"ntn_[A-Za-z0-9]{40,50}",
            0.97,
        )
        .check(check::random),
        r(
            "figma_pat",
            "Figma Token",
            "figd_ + 38-64 [A-Za-z0-9_-]",
            &["figd_"],
            r"figd_[A-Za-z0-9_-]{38,64}",
            0.98,
        ),
        r(
            "airtable_pat",
            "Airtable Token",
            "pat + 14 [A-Za-z0-9] + . + 64 hex",
            &["pat"],
            r"pat[A-Za-z0-9]{14}\.[a-f0-9]{64}",
            0.97,
        )
        .fwd(90),
        r(
            "dropbox_token",
            "Dropbox Token",
            "sl. + 130+ [A-Za-z0-9_=-]",
            &["sl."],
            r"sl\.(?:u\.)?[A-Za-z0-9_=-]{130,}",
            0.9,
        )
        .fwd(2048)
        .bounds(Bound::Base64, Bound::Base64)
        .check(check::random),
        r(
            "shopify_access_token",
            "Shopify Token",
            "shpat_ + 32 hex",
            &["shpat_"],
            r"shpat_[a-fA-F0-9]{32}",
            0.99,
        ),
        r(
            "shopify_custom_app_token",
            "Shopify Token",
            "shpca_ + 32 hex",
            &["shpca_"],
            r"shpca_[a-fA-F0-9]{32}",
            0.99,
        ),
        r(
            "shopify_private_app_token",
            "Shopify Token",
            "shppa_ + 32 hex",
            &["shppa_"],
            r"shppa_[a-fA-F0-9]{32}",
            0.99,
        ),
        r(
            "shopify_shared_secret",
            "Shopify Shared Secret",
            "shpss_ + 32 hex",
            &["shpss_"],
            r"shpss_[a-fA-F0-9]{32}",
            0.99,
        ),
        r(
            "square_access_token",
            "Square Token",
            "sq0atp- + 22 [A-Za-z0-9_-]",
            &["sq0atp-"],
            r"sq0atp-[A-Za-z0-9_-]{22}",
            0.99,
        ),
        r(
            "square_oauth_secret",
            "Square OAuth Secret",
            "sq0csp- + 43 [A-Za-z0-9_-]",
            &["sq0csp-"],
            r"sq0csp-[A-Za-z0-9_-]{43}",
            0.99,
        ),
        r(
            "stripe_secret_key",
            "Stripe Secret Key",
            "sk_live_/sk_test_ + 24+ [A-Za-z0-9]",
            &["sk_live_", "sk_test_"],
            r"sk_(?:live|test)_[A-Za-z0-9]{24,}",
            0.99,
        ),
        r(
            "stripe_restricted_key",
            "Stripe Restricted Key",
            "rk_live_/rk_test_ + 24+ [A-Za-z0-9]",
            &["rk_live_", "rk_test_"],
            r"rk_(?:live|test)_[A-Za-z0-9]{24,}",
            0.99,
        ),
        r(
            "stripe_webhook_secret",
            "Stripe Webhook Secret",
            "whsec_ + 32+ [A-Za-z0-9]",
            &["whsec_"],
            r"whsec_[A-Za-z0-9]{32,64}",
            0.98,
        ),
        r(
            "sendgrid_api_key",
            "SendGrid API Key",
            "SG. + 22 + . + 43 base64url",
            &["SG."],
            r"SG\.[A-Za-z0-9_-]{22}\.[A-Za-z0-9_-]{43,}",
            0.99,
        ),
        r(
            "mailgun_api_key",
            "Mailgun API Key",
            "key- + 32 hex (higher near \"mailgun\")",
            &["key-"],
            r"key-[0-9a-f]{32}",
            0.6,
        )
        .check(mailgun_context),
        r(
            "mailchimp_api_key",
            "Mailchimp API Key",
            "32 hex + -us + datacenter",
            &["-us"],
            r"[0-9a-f]{32}-us[0-9]{1,2}",
            0.9,
        )
        .back(32)
        .fwd(8),
        r(
            "twilio_account_sid",
            "Twilio Account SID",
            "AC + 32 hex",
            &["AC"],
            r"AC[0-9a-f]{32}",
            0.8,
        ),
        r(
            "twilio_api_key",
            "Twilio API Key",
            "SK + 32 hex",
            &["SK"],
            r"SK[0-9a-f]{32}",
            0.85,
        ),
        r(
            "telegram_bot_token",
            "Telegram Bot Token",
            "8-10 digits + :AA + 33 [A-Za-z0-9_-]",
            &[":AA"],
            r"[0-9]{8,10}:AA[A-Za-z0-9_-]{33}",
            0.97,
        )
        .back(10)
        .fwd(40),
        r(
            "discord_bot_token",
            "Discord Bot Token",
            "[MNO] + 23-25 + . + 6 + . + 27-38 base64url",
            &[],
            r"[MNO][A-Za-z0-9_-]{23,25}\.[A-Za-z0-9_-]{6}\.[A-Za-z0-9_-]{27,}",
            0.85,
        )
        .bounds(Bound::Base64, Bound::Base64)
        .check(check::random),
        r(
            "discord_webhook",
            "Discord Webhook URL",
            "https://discord.com/api/webhooks/<id>/<token>",
            &["/api/webhooks/"],
            r"https://(?:ptb\.|canary\.)?discord(?:app)?\.com/api/webhooks/[0-9]{17,20}/[A-Za-z0-9_-]{60,}",
            0.99,
        )
        .back(40)
        .fwd(120),
        r(
            "slack_bot_token",
            "Slack Token",
            "xoxb- + digits-digits- + 24-34 [A-Za-z0-9]",
            &["xoxb-"],
            r"xoxb-[0-9]{10,13}-[0-9]{10,13}-[A-Za-z0-9]{24,34}",
            0.99,
        ),
        r(
            "slack_user_token",
            "Slack Token",
            "xoxp- + 3 number groups + 32 hex",
            &["xoxp-"],
            r"xoxp-[0-9]{10,13}-[0-9]{10,13}-[0-9]{10,13}-[a-f0-9]{32,}",
            0.99,
        ),
        r(
            "slack_legacy_token",
            "Slack Token",
            "xoxa-/xoxr-/xoxs- + 10+ [A-Za-z0-9-]",
            &["xoxa-", "xoxr-", "xoxs-"],
            r"xox[ars]-[A-Za-z0-9-]{10,}",
            0.95,
        )
        .check(check::random),
        r(
            "slack_app_token",
            "Slack App Token",
            "xapp- + n- + id- + digits- + 32+ hex",
            &["xapp-"],
            r"xapp-[0-9]-[A-Z0-9]{9,12}-[0-9]{10,13}-[a-f0-9]{32,80}",
            0.99,
        ),
        r(
            "slack_config_token",
            "Slack Config Token",
            "xoxe[.xoxp]- + n- + 100+ [A-Za-z0-9]",
            &["xoxe"],
            r"xoxe(?:\.xox[bp])?-[0-9]-[A-Za-z0-9]{100,}",
            0.98,
        ),
        r(
            "slack_webhook",
            "Slack Webhook URL",
            "https://hooks.slack.com/services/…",
            &["hooks.slack.com/"],
            r"https://hooks\.slack\.com/(?:services|workflows|triggers)/[A-Za-z0-9+/_-]{40,}",
            0.99,
        )
        .back(8)
        .fwd(200),
        r(
            "zapier_webhook",
            "Zapier Webhook",
            "https://hooks.zapier.com/hooks/catch/<id>/<secret>/",
            &["hooks.zapier.com/hooks/catch/"],
            r"hooks\.zapier\.com/hooks/catch/[0-9]{3,12}/(?P<s>[A-Za-z0-9]{6,64})",
            0.95,
        )
        .fwd(100),
        r(
            "teams_webhook",
            "Microsoft Teams Webhook URL",
            "https://<tenant>.webhook.office.com/webhookb2/…",
            &["webhook.office.com/webhookb2/"],
            r"https://[A-Za-z0-9.-]{1,60}\.webhook\.office\.com/webhookb2/[A-Za-z0-9@/_-]{40,}",
            0.98,
        )
        .back(70)
        .fwd(500),
        r(
            "meta_access_token",
            "Meta Access Token",
            "EAA + 90+ [A-Za-z0-9]",
            &["EAA"],
            r"EAA[A-Za-z0-9]{90,}",
            0.85,
        )
        .fwd(1024)
        .bounds(Bound::Base64, Bound::Base64)
        .check(check::random),
        r(
            "firebase_fcm_server_key",
            "Firebase Cloud Messaging Key",
            "AAAA + 7 + :APA91b + 120-160 base64url",
            &[":APA91b"],
            r"AAAA[A-Za-z0-9_-]{7}:APA91b[A-Za-z0-9_-]{120,160}",
            0.98,
        )
        .back(11)
        .fwd(180),
        r(
            "onepassword_service_account_token",
            "1Password Service Account Token",
            "ops_eyJ + 100+ base64",
            &["ops_eyJ"],
            r"ops_eyJ[A-Za-z0-9+/=_-]{100,}",
            0.98,
        )
        .fwd(8192),
        r(
            "onepassword_secret_key",
            "1Password Secret Key",
            "A3-XXXXXX-XXXXXX-XXXXX-XXXXX-XXXXX-XXXXX",
            &["A3-"],
            r"A3-[A-Z0-9]{6}-(?:[A-Z0-9]{11}|[A-Z0-9]{6}-[A-Z0-9]{5})-[A-Z0-9]{5}-[A-Z0-9]{5}-[A-Z0-9]{5}",
            0.97,
        ),
        r(
            "tailscale_key",
            "Tailscale Key",
            "tskey-auth-/api-/client-/scim-/webhook- + id + - + secret",
            &[
                "tskey-auth-",
                "tskey-api-",
                "tskey-client-",
                "tskey-scim-",
                "tskey-webhook-",
            ],
            r"tskey-(?:auth|api|client|scim|webhook)-[A-Za-z0-9]{6,30}-[A-Za-z0-9]{20,64}",
            0.99,
        ),
        r(
            "age_secret_key",
            "age Secret Key",
            "AGE-SECRET-KEY-1 + 58 [0-9A-Z]",
            &["AGE-SECRET-KEY-1"],
            r"AGE-SECRET-KEY-1[0-9A-Z]{58}",
            0.99,
        ),
    ]
}
