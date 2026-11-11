//! One positive and one look-alike negative per catalog rule.
//!
//! Each test builds an engine with only the rule under test enabled, so a hit or a miss is
//! that rule's own doing. Tokens are generated at run time (see `support/gen.rs`).

mod support;

use redactor_core::{rule_infos, Engine, EngineConfig, ScanRequest, Sensitivity};
use support::gen::*;

/// An engine with every rule except `id` disabled.
fn only(id: &str) -> Engine {
    let disabled = rule_infos()
        .into_iter()
        .map(|r| r.id.to_string())
        .filter(|r| r != id)
        .collect();
    Engine::new(EngineConfig {
        disabled,
        enabled: vec![id.to_string()],
        pii: true,
        ..EngineConfig::default()
    })
    .unwrap()
}

fn assert_hit(id: &str, text: &str, secret: &str) {
    let pos = text
        .find(secret)
        .unwrap_or_else(|| panic!("{id}: test text must contain the secret"));
    let out = only(id).scan(
        text,
        &ScanRequest {
            sensitivity: Some(Sensitivity::Balanced),
            ..ScanRequest::default()
        },
    );
    let spans: Vec<(usize, usize, &str)> = out
        .findings
        .iter()
        .map(|f| (f.start, f.end, f.detector.as_str()))
        .collect();
    assert!(
        out.findings
            .iter()
            .any(|f| f.detector == id && f.start == pos && f.end == pos + secret.len()),
        "{id}: expected exactly [{pos}, {}) to be found, got {spans:?}",
        pos + secret.len()
    );
    assert!(
        !out.output.contains(secret),
        "{id}: the secret survived in the output"
    );
}

fn assert_miss(id: &str, text: &str) {
    let out = only(id).scan(
        text,
        &ScanRequest {
            sensitivity: Some(Sensitivity::Paranoid),
            ..ScanRequest::default()
        },
    );
    let hits: Vec<(usize, usize)> = out
        .findings
        .iter()
        .filter(|f| f.detector == id)
        .map(|f| (f.start, f.end))
        .collect();
    assert!(hits.is_empty(), "{id}: look-alike was flagged at {hits:?}");
}

/// `NAME=<secret>` on its own line.
fn env(s: String) -> (String, String) {
    (format!("export SOME_SETTING={s}\n"), s)
}

/// `before<secret>after`.
fn ctx(before: &str, s: String, after: &str) -> (String, String) {
    (format!("{before}{s}{after}"), s)
}

fn whole(s: String) -> (String, String) {
    (s.clone(), s)
}

macro_rules! rule_cases {
    ($( $hit:ident $miss:ident $id:literal |$g:ident| $pos:expr ; $neg:expr ; )*) => {
        $(
            #[test]
            fn $hit() {
                let mut $g = Gen::named(concat!($id, "+"));
                let (text, secret): (String, String) = $pos;
                assert_hit($id, &text, &secret);
            }

            #[test]
            #[allow(unused_mut, unused_variables)]
            fn $miss() {
                let mut $g = Gen::named(concat!($id, "-"));
                let text: String = $neg;
                assert_miss($id, &text);
            }
        )*

        const COVERED: &[&str] = &[$($id),*];
    };
}

rule_cases! {
    // ---- AI ----------------------------------------------------------------------------
    rule_elevenlabs_api_key_hits rule_elevenlabs_api_key_ignores_lookalike "elevenlabs_api_key"
        |g| ctx("xi-api-key header value ", format!("sk_{}", g.hex(48)), "\n");
        format!("sk_{}\n", g.hex(20));
    rule_openai_project_key_hits rule_openai_project_key_ignores_lookalike "openai_project_key"
        |g| env(format!("sk-proj-{}", g.alnum(60)));
        format!("OPENAI_API_KEY=sk-proj-{}\n", g.alnum(12));
    rule_openai_service_account_key_hits rule_openai_service_account_key_ignores_lookalike "openai_service_account_key"
        |g| env(format!("sk-svcacct-{}", g.alnum(60)));
        format!("key sk-svcacct-{}\n", g.alnum(12));
    rule_openai_admin_key_hits rule_openai_admin_key_ignores_lookalike "openai_admin_key"
        |g| env(format!("sk-admin-{}", g.alnum(60)));
        "see sk-admin-panel-settings-for-the-dashboard-page-layout\n".to_string();
    rule_openai_legacy_key_hits rule_openai_legacy_key_ignores_lookalike "openai_legacy_key"
        |g| env(format!("sk-{}T3BlbkFJ{}", g.alnum(20), g.alnum(20)));
        "the marker T3BlbkFJ alone is not a key\n".to_string();
    rule_openai_api_key_hits rule_openai_api_key_ignores_lookalike "openai_api_key"
        |g| env(format!("sk-{}", g.alnum(48)));
        format!("subtask-{}\n", g.alnum(40));
    rule_anthropic_api_key_hits rule_anthropic_api_key_ignores_lookalike "anthropic_api_key"
        |g| env(format!("sk-ant-api03-{}AA", g.alnum(93)));
        format!("sk-ant-api03-{}\n", g.alnum(20));
    rule_anthropic_admin_key_hits rule_anthropic_admin_key_ignores_lookalike "anthropic_admin_key"
        |g| env(format!("sk-ant-admin01-{}AA", g.alnum(93)));
        format!("sk-ant-admin01-{}\n", g.alnum(20));
    rule_anthropic_key_hits rule_anthropic_key_ignores_lookalike "anthropic_key"
        |g| env(format!("sk-ant-oat01-{}", g.alnum(80)));
        "sk-ant-colony-simulation-notes-for-the-weekend\n".to_string();
    rule_huggingface_token_hits rule_huggingface_token_ignores_lookalike "huggingface_token"
        |g| env(format!("hf_{}", g.alnum(34)));
        "from huggingface_hub import hf_hub_download\n".to_string();
    rule_groq_api_key_hits rule_groq_api_key_ignores_lookalike "groq_api_key"
        |g| env(format!("gsk_{}", g.alnum(52)));
        format!("gsk_{}\n", g.alnum(20));
    rule_replicate_api_token_hits rule_replicate_api_token_ignores_lookalike "replicate_api_token"
        |g| env(format!("r8_{}", g.alnum(37)));
        format!("r8_{}\n", g.alnum(10));
    rule_perplexity_api_key_hits rule_perplexity_api_key_ignores_lookalike "perplexity_api_key"
        |g| env(format!("pplx-{}", g.alnum(48)));
        format!("pplx-{}\n", g.alnum(30));
    rule_xai_api_key_hits rule_xai_api_key_ignores_lookalike "xai_api_key"
        |g| env(format!("xai-{}", g.alnum(80)));
        "model: xai-grok-beta\n".to_string();
    rule_openrouter_api_key_hits rule_openrouter_api_key_ignores_lookalike "openrouter_api_key"
        |g| env(format!("sk-or-v1-{}", g.hex(64)));
        format!("sk-or-v1-{}\n", g.hex(20));
    rule_langsmith_api_key_hits rule_langsmith_api_key_ignores_lookalike "langsmith_api_key"
        |g| env(format!("lsv2_pt_{}_{}", g.hex(32), g.hex(10)));
        format!("lsv2_pt_{}\n", g.hex(32));
    rule_pinecone_api_key_hits rule_pinecone_api_key_ignores_lookalike "pinecone_api_key"
        |g| env(format!("pcsk_{}_{}", g.alnum(6), g.alnum(63)));
        "pcsk_abc\n".to_string();
    rule_tavily_api_key_hits rule_tavily_api_key_ignores_lookalike "tavily_api_key"
        |g| env(format!("tvly-dev-{}", g.alnum(32)));
        format!("tvly-{}\n", g.alnum(10));
    rule_fireworks_api_key_hits rule_fireworks_api_key_ignores_lookalike "fireworks_api_key"
        |g| env(format!("fw_{}", g.alnum(24)));
        "fw_update_firmware_version_now\n".to_string();

    // ---- Code hosting and dev -----------------------------------------------------------
    rule_databricks_token_hits rule_databricks_token_ignores_lookalike "databricks_token"
        |g| ctx("databricks configure --token   # paste: ", format!("dapi{}", g.hex(32)), "\n");
        "dapi_version = 2\n".to_string();
    rule_github_pat_hits rule_github_pat_ignores_lookalike "github_pat"
        |g| env(g.github("ghp_"));
        format!("ghp_{}\n", g.alnum(20));
    rule_github_oauth_token_hits rule_github_oauth_token_ignores_lookalike "github_oauth_token"
        |g| env(g.github("gho_"));
        format!("gho_{}\n", g.alnum(20));
    rule_github_user_token_hits rule_github_user_token_ignores_lookalike "github_user_token"
        |g| env(g.github("ghu_"));
        format!("ghu_{}\n", g.alnum(20));
    rule_github_server_token_hits rule_github_server_token_ignores_lookalike "github_server_token"
        |g| env(g.github("ghs_"));
        format!("ghs_{}\n", g.alnum(20));
    rule_github_refresh_token_hits rule_github_refresh_token_ignores_lookalike "github_refresh_token"
        |g| env(g.github("ghr_"));
        format!("ghr_{}\n", g.alnum(20));
    rule_github_fine_grained_pat_hits rule_github_fine_grained_pat_ignores_lookalike "github_fine_grained_pat"
        |g| env(format!("github_pat_{}_{}", g.alnum(22), g.alnum(59)));
        format!("github_pat_{}\n", g.alnum(22));
    rule_gitlab_pat_hits rule_gitlab_pat_ignores_lookalike "gitlab_pat"
        |g| env(format!("glpat-{}", g.alnum(20)));
        format!("glpat-{}\n", g.alnum(10));
    rule_gitlab_deploy_token_hits rule_gitlab_deploy_token_ignores_lookalike "gitlab_deploy_token"
        |g| env(format!("gldt-{}", g.alnum(24)));
        format!("gldt-{}\n", g.alnum(8));
    rule_gitlab_runner_token_hits rule_gitlab_runner_token_ignores_lookalike "gitlab_runner_token"
        |g| env(format!("glrt-{}", g.alnum(24)));
        format!("glrt-{}\n", g.alnum(8));
    rule_gitlab_pipeline_trigger_token_hits rule_gitlab_pipeline_trigger_token_ignores_lookalike "gitlab_pipeline_trigger_token"
        |g| env(format!("glptt-{}", g.hex(40)));
        format!("glptt-{}\n", g.hex(20));
    rule_gitlab_scim_token_hits rule_gitlab_scim_token_ignores_lookalike "gitlab_scim_token"
        |g| env(format!("glsoat-{}", g.alnum(24)));
        format!("glsoat-{}\n", g.alnum(8));
    rule_gitlab_ci_build_token_hits rule_gitlab_ci_build_token_ignores_lookalike "gitlab_ci_build_token"
        |g| env(format!("glcbt-64_{}", g.alnum(24)));
        "glcbt-64_short\n".to_string();
    rule_gitlab_runner_registration_token_hits rule_gitlab_runner_registration_token_ignores_lookalike "gitlab_runner_registration_token"
        |g| env(format!("GR1348941{}", g.alnum(20)));
        format!("GR1348941{}\n", g.alnum(5));
    rule_bitbucket_app_password_hits rule_bitbucket_app_password_ignores_lookalike "bitbucket_app_password"
        |g| env(format!("ATBB{}", g.alnum(32)));
        format!("ATBB{}\n", g.alnum(10));
    rule_atlassian_api_token_hits rule_atlassian_api_token_ignores_lookalike "atlassian_api_token"
        |g| env(format!("ATATT3{}", g.alnum(180)));
        format!("ATATT3{}\n", g.alnum(40));
    rule_npm_token_hits rule_npm_token_ignores_lookalike "npm_token"
        |g| env(format!("npm_{}", g.alnum(36)));
        format!("pnpm_{}\n", g.alnum(36));
    rule_pypi_token_hits rule_pypi_token_ignores_lookalike "pypi_token"
        |g| env(format!("pypi-Ag{}", g.alnum(90)));
        format!("pypi-Ag{}\n", g.alnum(20));
    rule_rubygems_api_key_hits rule_rubygems_api_key_ignores_lookalike "rubygems_api_key"
        |g| env(format!("rubygems_{}", g.hex(48)));
        format!("rubygems_{}\n", g.hex(20));
    rule_nuget_api_key_hits rule_nuget_api_key_ignores_lookalike "nuget_api_key"
        |g| env(format!("oy2{}", g.from(LOWER_ALNUM, 43)));
        "oy2abcdefghij\n".to_string();
    rule_dockerhub_pat_hits rule_dockerhub_pat_ignores_lookalike "dockerhub_pat"
        |g| env(format!("dckr_pat_{}", g.alnum(27)));
        "dckr_pat_abc\n".to_string();
    rule_terraform_cloud_token_hits rule_terraform_cloud_token_ignores_lookalike "terraform_cloud_token"
        |g| env(format!("{}.atlasv1.{}", g.alnum(14), g.alnum(67)));
        format!("{}.atlasv1.{}\n", g.alnum(14), g.alnum(10));
    rule_vault_service_token_hits rule_vault_service_token_ignores_lookalike "vault_service_token"
        |g| env(format!("hvs.{}", g.alnum(90)));
        "hvs.example\n".to_string();
    rule_vault_batch_token_hits rule_vault_batch_token_ignores_lookalike "vault_batch_token"
        |g| env(format!("hvb.{}", g.alnum(90)));
        "hvb.short\n".to_string();
    rule_vault_recovery_token_hits rule_vault_recovery_token_ignores_lookalike "vault_recovery_token"
        |g| env(format!("hvr.{}", g.alnum(90)));
        "hvr.short\n".to_string();
    rule_doppler_token_hits rule_doppler_token_ignores_lookalike "doppler_token"
        |g| env(format!("dp.pt.{}", g.alnum(43)));
        format!("dp.pt.{}\n", g.alnum(10));
    rule_pulumi_token_hits rule_pulumi_token_ignores_lookalike "pulumi_token"
        |g| env(format!("pul-{}", g.hex(40)));
        format!("pul-{}\n", g.hex(10));
    rule_netlify_token_hits rule_netlify_token_ignores_lookalike "netlify_token"
        |g| env(format!("nfp_{}", g.alnum(36)));
        format!("nfp_{}\n", g.alnum(10));
    rule_heroku_api_key_hits rule_heroku_api_key_ignores_lookalike "heroku_api_key"
        |g| env(format!("HRKU-AA{}", g.alnum(58)));
        format!("HRKU-{}\n", g.alnum(10));
    rule_digitalocean_pat_hits rule_digitalocean_pat_ignores_lookalike "digitalocean_pat"
        |g| env(format!("dop_v1_{}", g.hex(64)));
        format!("dop_v1_{}\n", g.hex(32));
    rule_digitalocean_oauth_token_hits rule_digitalocean_oauth_token_ignores_lookalike "digitalocean_oauth_token"
        |g| env(format!("doo_v1_{}", g.hex(64)));
        format!("doo_v1_{}\n", g.hex(32));
    rule_digitalocean_refresh_token_hits rule_digitalocean_refresh_token_ignores_lookalike "digitalocean_refresh_token"
        |g| env(format!("dor_v1_{}", g.hex(64)));
        format!("dor_v1_{}\n", g.hex(32));
    rule_supabase_pat_hits rule_supabase_pat_ignores_lookalike "supabase_pat"
        |g| env(format!("sbp_{}", g.hex(40)));
        format!("sbp_{}\n", g.hex(12));
    rule_supabase_secret_key_hits rule_supabase_secret_key_ignores_lookalike "supabase_secret_key"
        |g| env(format!("sb_secret_{}", g.alnum(32)));
        "sb_secret_abc\n".to_string();
    rule_planetscale_token_hits rule_planetscale_token_ignores_lookalike "planetscale_token"
        |g| env(format!("pscale_tkn_{}", g.alnum(43)));
        format!("pscale_tkn_{}\n", g.alnum(10));
    rule_planetscale_password_hits rule_planetscale_password_ignores_lookalike "planetscale_password"
        |g| env(format!("pscale_pw_{}", g.alnum(43)));
        format!("pscale_pw_{}\n", g.alnum(10));
    rule_planetscale_oauth_token_hits rule_planetscale_oauth_token_ignores_lookalike "planetscale_oauth_token"
        |g| env(format!("pscale_oauth_{}", g.alnum(43)));
        format!("pscale_oauth_{}\n", g.alnum(10));
    rule_postman_api_key_hits rule_postman_api_key_ignores_lookalike "postman_api_key"
        |g| env(format!("PMAK-{}-{}", g.hex(24), g.hex(34)));
        format!("PMAK-{}\n", g.hex(24));
    rule_sentry_org_token_hits rule_sentry_org_token_ignores_lookalike "sentry_org_token"
        |g| env(format!("sntrys_eyJ{}", g.alnum(100)));
        format!("sntrys_eyJ{}\n", g.alnum(10));
    rule_sentry_user_token_hits rule_sentry_user_token_ignores_lookalike "sentry_user_token"
        |g| env(format!("sntryu_{}", g.hex(64)));
        format!("sntryu_{}\n", g.hex(20));
    rule_grafana_service_account_token_hits rule_grafana_service_account_token_ignores_lookalike "grafana_service_account_token"
        |g| env(format!("glsa_{}_{}", g.alnum(32), g.hex(8)));
        format!("glsa_{}\n", g.alnum(32));
    rule_grafana_cloud_token_hits rule_grafana_cloud_token_ignores_lookalike "grafana_cloud_token"
        |g| env(format!("glc_{}", g.alnum(64)));
        format!("glc_{}\n", g.alnum(10));
    rule_newrelic_user_key_hits rule_newrelic_user_key_ignores_lookalike "newrelic_user_key"
        |g| env(format!("NRAK-{}", g.from(UPPER_ALNUM, 27)));
        format!("NRAK-{}\n", g.from(UPPER_ALNUM, 10));
    rule_sonarqube_token_hits rule_sonarqube_token_ignores_lookalike "sonarqube_token"
        |g| env(format!("squ_{}", g.hex(40)));
        format!("squ_{}\n", g.hex(10));
    rule_circleci_pat_hits rule_circleci_pat_ignores_lookalike "circleci_pat"
        |g| env(format!("CCIPAT_{}_{}", g.alnum(22), g.hex(40)));
        format!("CCIPAT_{}\n", g.alnum(22));
    rule_buildkite_token_hits rule_buildkite_token_ignores_lookalike "buildkite_token"
        |g| env(format!("bkua_{}", g.hex(40)));
        format!("bkua_{}\n", g.hex(10));
    rule_linear_api_key_hits rule_linear_api_key_ignores_lookalike "linear_api_key"
        |g| env(format!("lin_api_{}", g.alnum(40)));
        format!("lin_api_{}\n", g.alnum(10));
    rule_linear_oauth_token_hits rule_linear_oauth_token_ignores_lookalike "linear_oauth_token"
        |g| env(format!("lin_oauth_{}", g.hex(64)));
        format!("lin_oauth_{}\n", g.hex(20));

    // ---- SaaS and messaging --------------------------------------------------------------
    rule_notion_integration_secret_hits rule_notion_integration_secret_ignores_lookalike "notion_integration_secret"
        |g| env(format!("secret_{}", g.alnum(43)));
        format!("client_secret_{}\n", g.alnum(43));
    rule_notion_token_hits rule_notion_token_ignores_lookalike "notion_token"
        |g| env(format!("ntn_{}", g.alnum(46)));
        format!("ntn_{}\n", g.alnum(10));
    rule_figma_pat_hits rule_figma_pat_ignores_lookalike "figma_pat"
        |g| env(format!("figd_{}", g.alnum(40)));
        format!("figd_{}\n", g.alnum(10));
    rule_airtable_pat_hits rule_airtable_pat_ignores_lookalike "airtable_pat"
        |g| env(format!("pat{}.{}", g.alnum(14), g.hex(64)));
        format!("pattern.{}\n", g.hex(64));
    rule_dropbox_token_hits rule_dropbox_token_ignores_lookalike "dropbox_token"
        |g| env(format!("sl.{}", g.alnum(140)));
        format!("sl.{}\n", g.alnum(20));
    rule_shopify_access_token_hits rule_shopify_access_token_ignores_lookalike "shopify_access_token"
        |g| env(format!("shpat_{}", g.hex(32)));
        format!("shpat_{}\n", g.hex(10));
    rule_shopify_custom_app_token_hits rule_shopify_custom_app_token_ignores_lookalike "shopify_custom_app_token"
        |g| env(format!("shpca_{}", g.hex(32)));
        format!("shpca_{}\n", g.hex(10));
    rule_shopify_private_app_token_hits rule_shopify_private_app_token_ignores_lookalike "shopify_private_app_token"
        |g| env(format!("shppa_{}", g.hex(32)));
        format!("shppa_{}\n", g.hex(10));
    rule_shopify_shared_secret_hits rule_shopify_shared_secret_ignores_lookalike "shopify_shared_secret"
        |g| env(format!("shpss_{}", g.hex(32)));
        format!("shpss_{}\n", g.hex(10));
    rule_square_access_token_hits rule_square_access_token_ignores_lookalike "square_access_token"
        |g| env(format!("sq0atp-{}", g.alnum(22)));
        format!("sq0atp-{}\n", g.alnum(8));
    rule_square_oauth_secret_hits rule_square_oauth_secret_ignores_lookalike "square_oauth_secret"
        |g| env(format!("sq0csp-{}", g.alnum(43)));
        format!("sq0csp-{}\n", g.alnum(8));
    rule_stripe_secret_key_hits rule_stripe_secret_key_ignores_lookalike "stripe_secret_key"
        |g| env(format!("sk_live_{}", g.alnum(30)));
        format!("sk_live_{}\n", g.alnum(10));
    rule_stripe_restricted_key_hits rule_stripe_restricted_key_ignores_lookalike "stripe_restricted_key"
        |g| env(format!("rk_test_{}", g.alnum(30)));
        "rk_test_short\n".to_string();
    rule_stripe_webhook_secret_hits rule_stripe_webhook_secret_ignores_lookalike "stripe_webhook_secret"
        |g| env(format!("whsec_{}", g.alnum(32)));
        format!("whsec_{}\n", g.alnum(10));
    rule_sendgrid_api_key_hits rule_sendgrid_api_key_ignores_lookalike "sendgrid_api_key"
        |g| env(format!("SG.{}.{}", g.alnum(22), g.alnum(43)));
        format!("SG.{}\n", g.alnum(22));
    rule_mailgun_api_key_hits rule_mailgun_api_key_ignores_lookalike "mailgun_api_key"
        |g| ctx("MAILGUN_API_KEY=", format!("key-{}", g.hex(32)), "\n");
        format!("key-{}\n", g.hex(10));
    rule_mailchimp_api_key_hits rule_mailchimp_api_key_ignores_lookalike "mailchimp_api_key"
        |g| env(format!("{}-us14", g.hex(32)));
        format!("value={}-us14\n", g.hex(20));
    rule_twilio_account_sid_hits rule_twilio_account_sid_ignores_lookalike "twilio_account_sid"
        |g| env(format!("AC{}", g.hex(32)));
        format!("AC{}\n", g.from(HEX_UPPER, 32));
    rule_twilio_api_key_hits rule_twilio_api_key_ignores_lookalike "twilio_api_key"
        |g| env(format!("SK{}", g.hex(32)));
        format!("SKU{}\n", g.hex(32));
    rule_telegram_bot_token_hits rule_telegram_bot_token_ignores_lookalike "telegram_bot_token"
        |g| env(format!("{}:AA{}", g.digits(9), g.alnum(33)));
        format!("{}:AA{}\n", g.digits(9), g.alnum(10));
    rule_discord_bot_token_hits rule_discord_bot_token_ignores_lookalike "discord_bot_token"
        |g| env(format!("M{}.{}.{}", g.alnum(23), g.alnum(6), g.alnum(27)));
        format!("M{}.{}\n", g.alnum(23), g.alnum(6));
    rule_discord_webhook_hits rule_discord_webhook_ignores_lookalike "discord_webhook"
        |g| env(format!("https://discord.com/api/webhooks/{}/{}", g.digits(18), g.alnum(68)));
        format!("https://discord.com/api/webhooks/{}/short\n", g.digits(18));
    rule_slack_bot_token_hits rule_slack_bot_token_ignores_lookalike "slack_bot_token"
        |g| env(format!("xoxb-{}-{}-{}", g.digits(12), g.digits(13), g.alnum(24)));
        format!("xoxb-{}\n", g.digits(5));
    rule_slack_user_token_hits rule_slack_user_token_ignores_lookalike "slack_user_token"
        |g| env(format!("xoxp-{}-{}-{}-{}", g.digits(12), g.digits(12), g.digits(12), g.hex(32)));
        format!("xoxp-{}\n", g.digits(12));
    rule_slack_legacy_token_hits rule_slack_legacy_token_ignores_lookalike "slack_legacy_token"
        |g| env(format!("xoxs-{}", g.alnum(40)));
        "xoxs-abc\n".to_string();
    rule_slack_client_token_hits rule_slack_client_token_ignores_lookalike "slack_client_token"
        |g| ctx("my slack session: ", format!("xoxc-{}-{}-{}-{}", g.digits(13), g.digits(13), g.digits(13), g.hex(128)), "\n");
        format!("xoxc-{}-{}\n", g.digits(13), g.hex(20));
    rule_hubspot_private_app_token_hits rule_hubspot_private_app_token_ignores_lookalike "hubspot_private_app_token"
        |g| ctx("hubspot: ", format!("pat-na1-{}-{}-{}-{}-{}", g.hex(8), g.hex(4), g.hex(4), g.hex(4), g.hex(12)), "\n");
        "pat-na1-team-review-notes\n".to_string();
    rule_slack_app_token_hits rule_slack_app_token_ignores_lookalike "slack_app_token"
        |g| env(format!("xapp-1-{}-{}-{}", g.from(UPPER_ALNUM, 11), g.digits(13), g.hex(64)));
        format!("xapp-1-{}\n", g.from(UPPER_ALNUM, 11));
    rule_slack_config_token_hits rule_slack_config_token_ignores_lookalike "slack_config_token"
        |g| env(format!("xoxe.xoxp-1-{}", g.alnum(140)));
        format!("xoxe-1-{}\n", g.alnum(20));
    rule_slack_webhook_hits rule_slack_webhook_ignores_lookalike "slack_webhook"
        |g| env(format!(
            "https://hooks.slack.com/services/T{}/B{}/{}",
            g.from(UPPER_ALNUM, 10),
            g.from(UPPER_ALNUM, 10),
            g.alnum(24)
        ));
        "see https://hooks.slack.com/services/ for setup\n".to_string();
    rule_zapier_webhook_hits rule_zapier_webhook_ignores_lookalike "zapier_webhook"
        |g| ctx("https://hooks.zapier.com/hooks/catch/1234567/", g.alnum(20), "/\n");
        "https://hooks.zapier.com/hooks/catch/\n".to_string();
    rule_teams_webhook_hits rule_teams_webhook_ignores_lookalike "teams_webhook"
        |g| env(format!(
            "https://example.webhook.office.com/webhookb2/{}-{}@{}/IncomingWebhook/{}/{}",
            g.hex(8),
            g.hex(12),
            g.hex(12),
            g.hex(32),
            g.hex(12)
        ));
        "https://example.webhook.office.com/webhookb2/abc\n".to_string();
    rule_meta_access_token_hits rule_meta_access_token_ignores_lookalike "meta_access_token"
        |g| env(format!("EAAB{}", g.alnum(150)));
        format!("EAA{}\n", g.alnum(30));
    rule_firebase_fcm_server_key_hits rule_firebase_fcm_server_key_ignores_lookalike "firebase_fcm_server_key"
        |g| env(format!("AAAA{}:APA91b{}", g.alnum(7), g.alnum(140)));
        format!("AAAA{}:APA91b{}\n", g.alnum(7), g.alnum(20));
    rule_onepassword_service_account_token_hits rule_onepassword_service_account_token_ignores_lookalike "onepassword_service_account_token"
        |g| env(format!("ops_eyJ{}", g.alnum(300)));
        format!("ops_eyJ{}\n", g.alnum(20));
    rule_onepassword_secret_key_hits rule_onepassword_secret_key_ignores_lookalike "onepassword_secret_key"
        |g| env(format!(
            "A3-{}-{}-{}-{}-{}-{}",
            g.from(UPPER_ALNUM, 6),
            g.from(UPPER_ALNUM, 6),
            g.from(UPPER_ALNUM, 5),
            g.from(UPPER_ALNUM, 5),
            g.from(UPPER_ALNUM, 5),
            g.from(UPPER_ALNUM, 5)
        ));
        format!("A3-{}-ABC\n", g.from(UPPER_ALNUM, 6));
    rule_tailscale_key_hits rule_tailscale_key_ignores_lookalike "tailscale_key"
        |g| env(format!("tskey-auth-{}-{}", g.alnum(16), g.alnum(32)));
        format!("tskey-auth-{}\n", g.alnum(5));
    rule_age_secret_key_hits rule_age_secret_key_ignores_lookalike "age_secret_key"
        |g| env(format!("AGE-SECRET-KEY-1{}", g.from(UPPER_ALNUM, 58)));
        format!("AGE-SECRET-KEY-1{}\n", g.from(UPPER_ALNUM, 20));

    // ---- Cloud ---------------------------------------------------------------------------
    rule_aws_access_key_id_hits rule_aws_access_key_id_ignores_lookalike "aws_access_key_id"
        |g| env(format!("AKIA{}", g.from(B32, 16)));
        format!("aws_access_key_id = AKIA{}{}\n", "IOSFODNN7", "EXAMPLE");
    rule_aws_secret_access_key_hits rule_aws_secret_access_key_ignores_lookalike "aws_secret_access_key"
        |g| ctx("aws_secret_access_key = ", g.from(B64, 40), "\n");
        format!("aws_secret_access_key = {}\n", g.alnum(20));
    rule_aws_session_token_hits rule_aws_session_token_ignores_lookalike "aws_session_token"
        |g| ctx("AWS_SESSION_TOKEN=", g.from(B64, 300), "\n");
        format!("AWS_SESSION_TOKEN={}\n", g.alnum(30));
    rule_aws_presigned_signature_hits rule_aws_presigned_signature_ignores_lookalike "aws_presigned_signature"
        |g| ctx(
            "https://bucket.s3.amazonaws.com/f?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Signature=",
            g.hex(64),
            "\n"
        );
        format!("header X-Amz-Signature={}\n", g.hex(64));
    rule_aws_presigned_credential_hits rule_aws_presigned_credential_ignores_lookalike "aws_presigned_credential"
        |g| ctx(
            "https://bucket.s3.amazonaws.com/f?X-Amz-Credential=",
            format!("AKIA{}%2F20240101%2Fus-east-1%2Fs3%2Faws4_request", g.from(B32, 16)),
            "&X-Amz-Date=20240101T000000Z\n"
        );
        "https://bucket.s3.amazonaws.com/f?X-Amz-Credential=short\n".to_string();
    rule_aws_presigned_security_token_hits rule_aws_presigned_security_token_ignores_lookalike "aws_presigned_security_token"
        |g| ctx("https://b.s3.amazonaws.com/f?X-Amz-Security-Token=", g.alnum(200), "&x=1\n");
        "https://b.s3.amazonaws.com/f?X-Amz-Security-Token=short\n".to_string();
    rule_google_api_key_hits rule_google_api_key_ignores_lookalike "google_api_key"
        |g| env(format!("AIza{}", g.alnum(35)));
        format!("AIza{}\n", g.alnum(20));
    rule_google_oauth_access_token_hits rule_google_oauth_access_token_ignores_lookalike "google_oauth_access_token"
        |g| env(format!("ya29.{}", g.alnum(120)));
        format!("ya29.{}\n", g.alnum(10));
    rule_google_oauth_refresh_token_hits rule_google_oauth_refresh_token_ignores_lookalike "google_oauth_refresh_token"
        |g| env(format!("1//0{}", g.alnum(100)));
        "1//0abc\n".to_string();
    rule_google_oauth_client_secret_hits rule_google_oauth_client_secret_ignores_lookalike "google_oauth_client_secret"
        |g| env(format!("GOCSPX-{}", g.alnum(28)));
        format!("GOCSPX-{}\n", g.alnum(10));
    rule_gcp_signed_url_signature_hits rule_gcp_signed_url_signature_ignores_lookalike "gcp_signed_url_signature"
        |g| ctx(
            "https://storage.googleapis.com/b/o?X-Goog-Algorithm=GOOG4-RSA-SHA256&X-Goog-Signature=",
            g.hex(512),
            "\n"
        );
        format!("https://storage.googleapis.com/b/o?X-Goog-Signature={}\n", g.hex(64));
    rule_azure_storage_account_key_hits rule_azure_storage_account_key_ignores_lookalike "azure_storage_account_key"
        |g| ctx(
            "DefaultEndpointsProtocol=https;AccountName=example;AccountKey=",
            format!("{}==", g.from(B64, 86)),
            ";EndpointSuffix=core.windows.net\n"
        );
        format!("AccountName=example;AccountKey={}\n", g.from(B64, 40));
    rule_azure_shared_access_key_hits rule_azure_shared_access_key_ignores_lookalike "azure_shared_access_key"
        |g| ctx(
            "Endpoint=sb://example.servicebus.windows.net/;SharedAccessKeyName=Root;SharedAccessKey=",
            format!("{}=", g.from(B64, 43)),
            "\n"
        );
        "Endpoint=sb://example.servicebus.windows.net/;SharedAccessKeyName=Root\n".to_string();
    rule_azure_sas_signature_hits rule_azure_sas_signature_ignores_lookalike "azure_sas_signature"
        |g| ctx(
            "https://example.blob.core.windows.net/c/f?sv=2022-11-02&ss=b&sp=r&se=2030-01-01&sig=",
            format!("{}%3D", g.alnum(43)),
            "\n"
        );
        format!("https://cdn.example.com/f?sig={}%3D\n", g.alnum(43));
    rule_azure_ad_client_secret_hits rule_azure_ad_client_secret_ignores_lookalike "azure_ad_client_secret"
        |g| env(format!("{}{}Q~{}", g.alnum(3), g.digits(1), g.alnum(34)));
        format!("{}{}Q~{}\n", g.alnum(3), g.digits(1), g.alnum(10));
    rule_azure_devops_pat_hits rule_azure_devops_pat_ignores_lookalike "azure_devops_pat"
        |g| ctx("AZURE_DEVOPS_PAT=", g.from(B32_LOWER, 52), "\n");
        "AZURE_DEVOPS_PAT=short\n".to_string();
    rule_azure_devops_pat_v2_hits rule_azure_devops_pat_v2_ignores_lookalike "azure_devops_pat_v2"
        |g| env(format!("{}AZDO{}", g.alnum(76), g.alnum(4)));
        format!("value={}AZDO{}\n", g.alnum(40), g.alnum(4));
    rule_azure_openai_key_hits rule_azure_openai_key_ignores_lookalike "azure_openai_key"
        |g| ctx("AZURE_OPENAI_API_KEY=", g.hex(32), "\n");
        format!("AZURE_OPENAI_API_KEY={}\n", g.hex(10));
    rule_azure_cosmos_key_hits rule_azure_cosmos_key_ignores_lookalike "azure_cosmos_key"
        |g| ctx("COSMOS_KEY=", format!("{}==", g.from(B64, 86)), "\n");
        format!("COSMOS_KEY={}\n", g.from(B64, 20));
    rule_alibaba_access_key_id_hits rule_alibaba_access_key_id_ignores_lookalike "alibaba_access_key_id"
        |g| env(format!("LTAI{}", g.alnum(20)));
        format!("LTAI{}\n", g.alnum(5));
    rule_tencent_secret_id_hits rule_tencent_secret_id_ignores_lookalike "tencent_secret_id"
        |g| env(format!("AKID{}", g.alnum(32)));
        format!("AKID{}\n", g.alnum(5));
    rule_cloudflare_api_token_hits rule_cloudflare_api_token_ignores_lookalike "cloudflare_api_token"
        |g| ctx("CLOUDFLARE_API_TOKEN=", g.alnum(40), "\n");
        format!("CLOUDFLARE_API_TOKEN={}\n", g.alnum(12));
    rule_cloudflare_origin_ca_key_hits rule_cloudflare_origin_ca_key_ignores_lookalike "cloudflare_origin_ca_key"
        |g| env(format!("v1.0-{}-{}", g.hex(24), g.hex(146)));
        format!("v1.0-{}\n", g.hex(24));

    // ---- Keys and crypto material ---------------------------------------------------------
    rule_jwt_hits rule_jwt_ignores_lookalike "jwt"
        |g| ctx("Authorization header was ", g.jwt(), "\n");
        format!(
            "{}.{}.{}\n",
            b64url_encode(br#"{"foo":"bar","n":1}"#),
            b64url_encode(br#"{"sub":"x","iat":1}"#),
            b64url_encode(&g.bytes(32))
        );
    rule_bitcoin_wif_hits rule_bitcoin_wif_ignores_lookalike "bitcoin_wif"
        |g| env(g.wif());
        {
            let mut w = g.wif();
            let last = w.pop().unwrap();
            w.push(if last == 'z' { 'y' } else { 'z' });
            format!("key {w}\n")
        };
    rule_wireguard_private_key_hits rule_wireguard_private_key_ignores_lookalike "wireguard_private_key"
        |g| ctx("[Interface]\nPrivateKey = ", format!("{}A=", g.from(B64, 42)), "\n");
        format!("PrivateKey = {}=\n", g.from(B64, 10));
    rule_kubeconfig_client_key_data_hits rule_kubeconfig_client_key_data_ignores_lookalike "kubeconfig_client_key_data"
        |g| ctx("    client-key-data: ", g.from(B64, 400), "\n");
        "    client-key-data: abc\n".to_string();
    rule_private_key_hits rule_private_key_ignores_lookalike "private_key"
        |g| ctx(
            "key:\n",
            format!(
                "-----BEGIN RSA PRIVATE KEY-----\n{}\n{}\n-----END RSA PRIVATE KEY-----",
                g.from(B64, 64),
                g.from(B64, 40)
            ),
            "\n"
        );
        format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
            g.from(B64, 64)
        );
    rule_pgp_private_key_hits rule_pgp_private_key_ignores_lookalike "pgp_private_key"
        |g| whole(format!(
            "-----BEGIN PGP PRIVATE KEY BLOCK-----\n\n{}\n={}\n-----END PGP PRIVATE KEY BLOCK-----",
            g.from(B64, 64),
            g.from(B64, 4)
        ));
        format!(
            "-----BEGIN PGP PUBLIC KEY BLOCK-----\n\n{}\n-----END PGP PUBLIC KEY BLOCK-----\n",
            g.from(B64, 64)
        );
    rule_openvpn_static_key_hits rule_openvpn_static_key_ignores_lookalike "openvpn_static_key"
        |g| whole(format!(
            "-----BEGIN OpenVPN Static key V1-----\n{}\n{}\n-----END OpenVPN Static key V1-----",
            g.hex(32),
            g.hex(32)
        ));
        "the file starts with an OpenVPN Static key V1 header\n".to_string();
    rule_ssh2_private_key_hits rule_ssh2_private_key_ignores_lookalike "ssh2_private_key"
        |g| whole(format!(
            "---- BEGIN SSH2 ENCRYPTED PRIVATE KEY ----\nComment: \"key\"\n{}\n---- END SSH2 ENCRYPTED PRIVATE KEY ----",
            g.from(B64, 64)
        ));
        format!(
            "---- BEGIN SSH2 PUBLIC KEY ----\n{}\n---- END SSH2 PUBLIC KEY ----\n",
            g.from(B64, 64)
        );
    rule_putty_private_key_hits rule_putty_private_key_ignores_lookalike "putty_private_key"
        |g| ctx("", putty(&mut g), "\n");
        "PuTTY-User-Key-File-3 is the first line of a .ppk file\n".to_string();
    rule_bip39_seed_phrase_hits rule_bip39_seed_phrase_ignores_lookalike "bip39_seed_phrase"
        |g| ctx("my seed: ", g.bip39(12, true), "\n");
        "abandon ability able about above absent absorb abstract absurd abuse access\n".to_string();

    // ---- Password hashes ------------------------------------------------------------------
    rule_bcrypt_hash_hits rule_bcrypt_hash_ignores_lookalike "bcrypt_hash"
        |g| env(format!("$2b$12${}", g.from(CRYPT, 53)));
        format!("$2b$12${}\n", g.from(CRYPT, 20));
    rule_argon2_hash_hits rule_argon2_hash_ignores_lookalike "argon2_hash"
        |g| env(format!(
            "$argon2id$v=19$m=65536,t=3,p=4${}${}",
            g.alnum(22),
            g.alnum(43)
        ));
        "$argon2id$v=19$m=65536\n".to_string();
    rule_sha512_crypt_hash_hits rule_sha512_crypt_hash_ignores_lookalike "sha512_crypt_hash"
        |g| env(format!("$6${}${}", g.from(CRYPT, 16), g.from(CRYPT, 86)));
        format!("$6${}${}\n", g.from(CRYPT, 16), g.from(CRYPT, 20));
    rule_sha256_crypt_hash_hits rule_sha256_crypt_hash_ignores_lookalike "sha256_crypt_hash"
        |g| env(format!("$5${}${}", g.from(CRYPT, 16), g.from(CRYPT, 43)));
        format!("$5${}${}\n", g.from(CRYPT, 16), g.from(CRYPT, 20));
    rule_md5_crypt_hash_hits rule_md5_crypt_hash_ignores_lookalike "md5_crypt_hash"
        |g| env(format!("$1${}${}", g.from(CRYPT, 8), g.from(CRYPT, 22)));
        format!("$1${}${}\n", g.from(CRYPT, 8), g.from(CRYPT, 10));
    rule_apr1_hash_hits rule_apr1_hash_ignores_lookalike "apr1_hash"
        |g| env(format!("$apr1${}${}", g.from(CRYPT, 8), g.from(CRYPT, 22)));
        format!("$apr1${}${}\n", g.from(CRYPT, 8), g.from(CRYPT, 10));
    rule_yescrypt_hash_hits rule_yescrypt_hash_ignores_lookalike "yescrypt_hash"
        |g| env(format!("$y$j9T${}${}", g.from(CRYPT, 22), g.from(CRYPT, 43)));
        format!("$y$j9T${}\n", g.from(CRYPT, 10));
    rule_pbkdf2_django_hash_hits rule_pbkdf2_django_hash_ignores_lookalike "pbkdf2_django_hash"
        |g| env(format!("pbkdf2_sha256$600000${}${}", g.alnum(22), g.from(B64, 44)));
        format!("pbkdf2_sha256$600000${}\n", g.alnum(22));
    rule_pwdump_hash_hits rule_pwdump_hash_ignores_lookalike "pwdump_hash"
        |g| ctx("Administrator:500:", format!("{}:{}", g.hex(32), g.hex(32)), ":::\n");
        format!("Administrator:500:{}:::\n", g.hex(32));
    rule_htpasswd_sha_hits rule_htpasswd_sha_ignores_lookalike "htpasswd_sha"
        |g| ctx("admin:", format!("{{SHA}}{}=", g.from(B64, 27)), "\n");
        format!("admin:{{SHA}}{}\n", g.from(B64, 10));

    // ---- HTTP, URLs, connection strings -----------------------------------------------------
    rule_auth_bearer_hits rule_auth_bearer_ignores_lookalike "auth_bearer"
        |g| ctx("Authorization: Bearer ", g.alnum(40), "\n");
        "Authorization: Bearer <token>\n".to_string();
    rule_auth_basic_hits rule_auth_basic_ignores_lookalike "auth_basic"
        |g| ctx(
            "Authorization: Basic ",
            b64_encode(format!("user:{}", g.alnum(12)).as_bytes()),
            "\n"
        );
        "Authorization: Basic abc\n".to_string();
    rule_auth_token_scheme_hits rule_auth_token_scheme_ignores_lookalike "auth_token_scheme"
        |g| ctx("Authorization: token ", g.alnum(40), "\n");
        "Authorization: Token ab\n".to_string();
    rule_auth_digest_response_hits rule_auth_digest_response_ignores_lookalike "auth_digest_response"
        |g| ctx(
            "Authorization: Digest username=\"u\", realm=\"r\", nonce=\"n\", uri=\"/\", response=\"",
            g.hex(32),
            "\"\n"
        );
        "Authorization: Digest username=\"u\", realm=\"r\"\n".to_string();
    rule_auth_aws4_signature_hits rule_auth_aws4_signature_ignores_lookalike "auth_aws4_signature"
        |g| ctx(
            "Authorization: AWS4-HMAC-SHA256 Credential=x/20240101/us-east-1/s3/aws4_request, SignedHeaders=host, Signature=",
            g.hex(64),
            "\n"
        );
        "Authorization: AWS4-HMAC-SHA256 Credential=x, SignedHeaders=host\n".to_string();
    rule_auth_ntlm_hits rule_auth_ntlm_ignores_lookalike "auth_ntlm"
        |g| ctx("Authorization: NTLM ", g.from(B64, 60), "\n");
        format!("WWW-Authenticate: NTLM {}\n", g.from(B64, 60));
    rule_bearer_token_hits rule_bearer_token_ignores_lookalike "bearer_token"
        |g| ctx("call it with Bearer ", g.alnum(40), " please\n");
        "the Bearer token type is described in RFC 6750\n".to_string();
    rule_api_key_header_hits rule_api_key_header_ignores_lookalike "api_key_header"
        |g| ctx("curl -H 'X-Api-Key: ", g.alnum(32), "' https://api.example.com\n");
        "X-Api-Key: <your-key>\n".to_string();
    rule_auth_token_header_hits rule_auth_token_header_ignores_lookalike "auth_token_header"
        |g| ctx("PRIVATE-TOKEN: ", g.alnum(20), "\n");
        "PRIVATE-TOKEN: abc\n".to_string();
    rule_cookie_header_hits rule_cookie_header_ignores_lookalike "cookie_header"
        |g| ctx("Cookie: ", format!("session={}; theme=dark", g.alnum(32)), "\n");
        "Set-Cookie: theme=dark\n".to_string();
    rule_set_cookie_header_hits rule_set_cookie_header_ignores_lookalike "set_cookie_header"
        |g| ctx("Set-Cookie: sid=", g.alnum(32), "; Path=/; HttpOnly\n");
        "Set-Cookie: theme=dark; Path=/\n".to_string();
    rule_session_id_hits rule_session_id_ignores_lookalike "session_id"
        |g| ctx("PHPSESSID=", g.alnum(26), "\n");
        "PHPSESSID=deleted\n".to_string();
    rule_url_userinfo_password_hits rule_url_userinfo_password_ignores_lookalike "url_userinfo_password"
        |g| ctx("postgres://app:", g.alnum(16), "@db.example.com:5432/app\n");
        "postgres://app:${DB_PASSWORD}@db.example.com/app\n".to_string();
    rule_jdbc_oracle_password_hits rule_jdbc_oracle_password_ignores_lookalike "jdbc_oracle_password"
        |g| ctx("jdbc:oracle:thin:scott/", g.alnum(16), "@//ora.example.com:1521/ORCL\n");
        "jdbc:oracle:thin:@//ora.example.com:1521/ORCL\n".to_string();
    rule_url_query_token_hits rule_url_query_token_ignores_lookalike "url_query_token"
        |g| ctx("https://example.com/cb?access_token=", g.alnum(40), "&state=xyz\n");
        "https://example.com/cb?token=abc\n".to_string();
    rule_url_query_client_secret_hits rule_url_query_client_secret_ignores_lookalike "url_query_client_secret"
        |g| ctx("https://example.com/oauth?client_id=abc&client_secret=", g.alnum(24), "\n");
        format!("the client_secret={} is in the docs\n", g.alnum(24));
    rule_url_query_key_hits rule_url_query_key_ignores_lookalike "url_query_key"
        |g| ctx("https://maps.example.com/api?key=", g.alnum(39), "&q=x\n");
        "https://maps.example.com/api?key=value\n".to_string();
    rule_url_query_password_hits rule_url_query_password_ignores_lookalike "url_query_password"
        |g| ctx("https://example.com/login?user=bob&password=", g.alnum(12), "\n");
        "https://example.com/reset?password_policy=strict\n".to_string();
    rule_url_query_signature_hits rule_url_query_signature_ignores_lookalike "url_query_signature"
        |g| ctx("https://cdn.example.com/f.png?expires=1&signature=", g.alnum(40), "\n");
        "https://cdn.example.com/f.png?signature=short\n".to_string();
    rule_connection_string_password_hits rule_connection_string_password_ignores_lookalike "connection_string_password"
        |g| ctx("Server=db.example.com;Database=app;User Id=app;Password=", g.alnum(16), ";\n");
        format!("Password={}\n", g.alnum(16));
    rule_pgpass_line_hits rule_pgpass_line_ignores_lookalike "pgpass_line"
        |g| ctx("db.example.com:5432:app:appuser:", g.alnum(16), "\n");
        "12:30:45:678:9000\n".to_string();
    rule_docker_config_auth_hits rule_docker_config_auth_ignores_lookalike "docker_config_auth"
        |g| ctx(
            "{\"auths\":{\"registry.example.com\":{\"auth\":\"",
            b64_encode(format!("user:{}", g.alnum(16)).as_bytes()),
            "\"}}}\n"
        );
        "{\"auth\": \"abc\"}\n".to_string();
    rule_npmrc_auth_hits rule_npmrc_auth_ignores_lookalike "npmrc_auth"
        |g| ctx("//registry.npmjs.org/:_authToken=", g.alnum(36), "\n");
        "//registry.npmjs.org/:_authToken=${NPM_TOKEN}\n".to_string();

    // ---- PII --------------------------------------------------------------------------------
    rule_email_hits rule_email_ignores_lookalike "email"
        |g| ctx("contact: ", format!("jane.{}@example.com", g.from(LOWER_ALNUM, 6)), "\n");
        "ssh user@localhost\n".to_string();
    rule_phone_e164_hits rule_phone_e164_ignores_lookalike "phone_e164"
        |g| ctx("call ", format!("+1 415 {} {}", g.digits(3), g.digits(4)), " today\n");
        "version +1.2\n".to_string();
    rule_phone_de_hits rule_phone_de_ignores_lookalike "phone_de"
        |g| ctx("Tel. ", format!("030 {}", g.digits(7)), "\n");
        "Order 0 12\n".to_string();
    rule_iban_hits rule_iban_ignores_lookalike "iban"
        |g| ctx("IBAN ", g.iban_de(), "\n");
        format!("IBAN DE00{}\n", g.digits(18));
    rule_credit_card_hits rule_credit_card_ignores_lookalike "credit_card"
        |g| ctx("card ", g.card("4", 16), "\n");
        {
            let c = g.card("4", 16);
            let last = c.as_bytes()[15] - b'0';
            format!("card {}{}\n", &c[..15], (last + 1) % 10)
        };
    rule_ipv4_hits rule_ipv4_ignores_lookalike "ipv4"
        |g| ctx("host ", format!("192.168.{}.{}", 1 + g.below(250), 1 + g.below(250)), " up\n");
        "version 1.2.3.4.5\n".to_string();
    rule_ipv6_hits rule_ipv6_ignores_lookalike "ipv6"
        |g| ctx("addr ", format!("2001:db8:{}::8a2e:370:7334", g.hex(4)), "\n");
        "time 12:30:45\n".to_string();
    rule_us_ssn_hits rule_us_ssn_ignores_lookalike "us_ssn"
        |g| ctx(
            "SSN ",
            format!("{}-{}-{}", 100 + g.below(500), 10 + g.below(89), 1000 + g.below(8999)),
            "\n"
        );
        "SSN 000-12-3456\n".to_string();

    // ---- Context engine (one entry; its own cases are in tests/context.rs) ---------------
    rule_context_hits rule_context_ignores_lookalike "context"
        |g| ctx("DB_PASSWORD=", format!("Fake-{}", g.alnum(12)), "\n");
        format!("bypass: Fake-{}\npassword_min_length: 12\n", g.alnum(12));

    // ---- Classifier (standalone unlabeled tokens; more in tests/classifier.rs) -------------
    rule_classifier_hits rule_classifier_ignores_lookalike "classifier"
        |g| ctx("the build printed ", g.alnum(32), " and stopped\n");
        format!(
            "fn getUserAccountSettings(node_modules: &Path) -> AbstractSingletonProxyFactoryBean\ncommit {}\n",
            g.hex(40)
        );
}

/// A PuTTY v3 key file with one private line and a MAC.
fn putty(g: &mut Gen) -> String {
    format!(
        "PuTTY-User-Key-File-3: ssh-ed25519\nEncryption: none\nComment: test\nPublic-Lines: 2\n{}\n{}\nPrivate-Lines: 1\n{}\nPrivate-MAC: {}",
        g.from(B64, 64),
        g.from(B64, 20),
        g.from(B64, 44),
        g.hex(64)
    )
}

#[test]
fn every_rule_has_cases() {
    let ids: Vec<&str> = rule_infos().iter().map(|r| r.id).collect();
    let missing: Vec<&&str> = ids.iter().filter(|id| !COVERED.contains(id)).collect();
    assert!(missing.is_empty(), "rules without tests: {missing:?}");
    let unknown: Vec<&&str> = COVERED.iter().filter(|id| !ids.contains(id)).collect();
    assert!(unknown.is_empty(), "tests for unknown rules: {unknown:?}");
}

/// The `.npmrc` rule needs the `.npmrc` shape: the setting at a line start or after a
/// registry scope's `:`. A member named `_password` in code is not one.
#[test]
fn npmrc_auth_needs_npmrc_shape() {
    let token = concat!("Npm", "Fake", "Token", "0123456789abcdef");
    assert_hit(
        "npmrc_auth",
        &format!("//registry.npmjs.org/:_authToken={token}\n"),
        token,
    );
    assert_hit("npmrc_auth", &format!("_auth={token}\n"), token);
    assert_hit("npmrc_auth", &format!("  _password = {token}\n"), token);
    assert_hit(
        "npmrc_auth",
        &format!("//npm.example.com/repo/:_password={token}\n"),
        token,
    );
    assert_miss("npmrc_auth", "self._password = password_value\n");
    assert_miss("npmrc_auth", "this._authToken = response.accessToken;\n");
    assert_miss("npmrc_auth", "    obj._auth = makeAuthHeader(user)\n");
}

/// A URL password may hold `/`, `?` or `#`; a port before a path is no password. A long
/// random userinfo is a token; an Oracle thin login and a Zapier hook carry secrets too.
#[test]
fn url_credentials_beyond_user_colon_password() {
    for (text, secret) in [
        (
            "DATABASE_URL=postgres://app:Xy7#kL9mQ2@db.internal:5432/prod\n",
            "Xy7#kL9mQ2",
        ),
        (
            "DATABASE_URL=postgres://app:Xy7/kL9mQ2@db.internal:5432/prod\n",
            "Xy7/kL9mQ2",
        ),
        (
            "REDIS_URL=redis://:Xy7?kL9mQ2@cache.internal:6379/0\n",
            "Xy7?kL9mQ2",
        ),
        (
            "git remote set-url origin https://Zq81mXv0pLr3TfK9wQe2Lm4x@git.example.com/team/repo.git\n",
            "Zq81mXv0pLr3TfK9wQe2Lm4x",
        ),
    ] {
        assert_hit("url_userinfo_password", text, secret);
    }
    assert_hit(
        "jdbc_oracle_password",
        "jdbc:oracle:thin:scott/Hunter2-Fake-9x@//ora.internal:1521/ORCLPDB1\n",
        "Hunter2-Fake-9x",
    );
    assert_hit(
        "zapier_webhook",
        "https://hooks.zapier.com/hooks/catch/1234567/k3j9x7q2m5n8p4w1v6b2/\n",
        "k3j9x7q2m5n8p4w1v6b2",
    );
    assert_hit(
        "url_query_token",
        "https://app.example.com/reset?reset_token=k3j9x7q2m5n8p4w1\n",
        "k3j9x7q2m5n8p4w1",
    );
    for text in [
        "https://example.com:8443/users/@me\n",
        "http://localhost:3000/@vite/client\n",
        "https://jdoe@dev.azure.com/org/proj/_git/repo\n",
        "https://medium.com/@jdoe/how-we-ship\n",
    ] {
        assert_miss("url_userinfo_password", text);
    }
    assert_miss(
        "url_query_token",
        "https://app.example.com/items?page_token=CAESBgoECAEQAQ\n",
    );
}

/// The widened userinfo scan stops at a character boundary: a multi-byte character where its
/// byte cap falls does not panic.
#[test]
fn url_userinfo_scan_cap_respects_char_boundaries() {
    for pad in 0..4 {
        let text = format!(
            "postgres://app:x/{}{}\n",
            "a".repeat(250 + pad),
            "é".repeat(8)
        );
        let _ = only("url_userinfo_password").scan(&text, &ScanRequest::default());
    }
}

/// A JSON or C escape (`\n`, `\t`) or an ANSI colour code glued before a token is a
/// boundary, not a word character; `\\n` (an escaped backslash, then `n`) is not.
#[test]
fn escapes_and_colour_codes_bound_a_token() {
    let mut g = Gen::named("escapes");
    let pat = g.github("ghp_");
    for (pre, post) in [
        ("{\"log\": \"cloning with token\\n", "\\n\"}"),
        ("{\"log\": \"step\\t", "\\tdone\"}"),
        ("\x1b[1m", "\x1b[0m"),
        ("\x1b[1;33m", "\x1b[0m"),
        ("printf \"\\033[32m", "\\033[0m\""),
    ] {
        assert_hit("github_pat", &format!("{pre}{pat}{post}\n"), &pat);
    }
    let jwt = g.jwt();
    assert_hit(
        "jwt",
        &format!("{{\"msg\": \"issued token:\\n{jwt}\"}}\n"),
        &jwt,
    );
    assert_miss("github_pat", &format!("path\\\\n{pat}x\n"));
}

/// JWE (five parts, the key part empty for direct encryption) and a JWT wrapped after its
/// dots are one token; a quote inside a connection-string password is part of it.
#[test]
fn jwe_wrapped_jwt_and_quoted_connection_passwords() {
    let mut g = Gen::named("jwe");
    let header = b64url_encode(br#"{"alg":"RSA-OAEP","enc":"A256GCM"}"#);
    let jwe = format!(
        "{header}.{}.{}.{}.{}",
        g.alnum(64),
        g.alnum(16),
        g.alnum(40),
        g.alnum(22)
    );
    assert_hit("jwt", &format!("token: {jwe}\n"), &jwe);
    let dir = b64url_encode(br#"{"alg":"dir","enc":"A256GCM"}"#);
    let direct = format!("{dir}..{}.{}.{}", g.alnum(16), g.alnum(40), g.alnum(22));
    assert_hit("jwt", &format!("token: {direct}\n"), &direct);
    let jwt = g.jwt();
    let wrapped = jwt.replace('.', ".\n ");
    assert_hit(
        "jwt",
        &format!("Authorization: Bearer {wrapped}\n"),
        &wrapped,
    );
    assert_hit(
        "connection_string_password",
        "Server=db1;Database=shop;User Id=sa;Password=P@ss'word1;\n",
        "P@ss'word1",
    );
    assert_hit(
        "connection_string_password",
        "Server=db1;Database=shop;User Id=sa;Password='Kiwi-2024x';\n",
        "Kiwi-2024x",
    );
    assert_miss(
        "connection_string_password",
        "Server=db1;Database=shop;User Id=sa;Password=${DB_PASSWORD};\n",
    );
}

/// A connection-string password sits in a `key=value;key=value` chain with a connection key;
/// statements joined by `;` and values that code reads are no chain.
#[test]
fn connection_string_password_needs_a_chain() {
    for (text, secret) in [
        (
            "Host=db.internal;Port=5432;Username=app;Password=Quill-Mantis-38;\n",
            "Quill-Mantis-38",
        ),
        (
            "var cs = \"Server=10.0.4.30;Database=crm;User Id=crm_app;Password=Otter-Plank-64;\";\n",
            "Otter-Plank-64",
        ),
        (
            "Password=Fennel-Brook-27;Server=10.0.4.31;Database=hr;\n",
            "Fennel-Brook-27",
        ),
        (
            "jdbc:sqlserver://10.0.4.33:1433;databaseName=pay;user=pay_app;password=Sorrel-Kite-19;\n",
            "Sorrel-Kite-19",
        ),
    ] {
        assert_hit("connection_string_password", text, secret);
    }
    for text in [
        "const user=req.body.user;const password=req.body.password;\n",
        "String host=cfg.getHost(); String password=cfg.getPassword();\n",
        "$user=$_POST['user'];$password=$_POST['password'];\n",
        "var user=getUser();var pwd=getPwd();\n",
        "host=args.host;user=args.user;password=args.password\n",
        "let host = opts.host; let password = opts.password;\n",
    ] {
        assert_miss("connection_string_password", text);
    }
}

/// `;` and `&` separate query parameters only in a URL (a `?` or `://` earlier in the token)
/// or a form body of `key=value&key=value` pairs; statements of code are neither.
#[test]
fn query_separators_need_a_url_or_form_body() {
    let mut g = Gen::named("query-sep");
    let tok = g.alnum(24);
    assert_hit(
        "url_query_token",
        &format!("https://files.example.com/get?id=7;token={tok}\n"),
        &tok,
    );
    assert_hit(
        "url_query_password",
        "curl https://auth.example.com/token -d 'grant_type=password&username=svc&password=Thistle-Canoe-58'\n",
        "Thistle-Canoe-58",
    );
    assert_hit(
        "url_query_client_secret",
        "client_id=web&client_secret=Vq7Lm2Xr9Kd4Tz8Wn3Hp&grant_type=client_credentials\n",
        "Vq7Lm2Xr9Kd4Tz8Wn3Hp",
    );
    for (id, text) in [
        (
            "url_query_token",
            "var a=1;code=e.keyCode||e.which;if(code===13){submit()}\n",
        ),
        ("url_query_token", "x=1;token=this.getToken();fetch(u)\n"),
        ("url_query_token", "if(ok&&code==200){done()}\n"),
        ("url_query_password", "if(a&&pass==b){ok()}\n"),
        (
            "url_query_password",
            "var u=1;pwd=document.getElementById(\"pwd\").value;\n",
        ),
        (
            "url_query_client_secret",
            "n=1;client_secret=read_secret();\n",
        ),
    ] {
        assert_miss(id, text);
    }
}

/// A Docker `"auth"` is base64 of `user:password` or a random token; a scheme name in an
/// `"auth"` field of some other JSON is neither.
#[test]
fn docker_auth_needs_credentials() {
    let mut g = Gen::named("docker-auth");
    let pair = b64_encode(format!("ci-bot:{}", g.alnum(18)).as_bytes());
    assert_hit(
        "docker_config_auth",
        &format!("{{\"auths\":{{\"ghcr.io\":{{\"auth\":\"{pair}\"}}}}}}\n"),
        &pair,
    );
    assert_hit(
        "docker_config_auth",
        "{\"auths\": {\"cr.example.com\": {\"auth\": \"Hx4mKq9Zp2Lw7Rt5Vb8N\"}}}\n",
        "Hx4mKq9Zp2Lw7Rt5Vb8N",
    );
    for text in [
        "{\"endpoint\": \"/v1/items\", \"auth\": \"required\", \"method\": \"GET\"}\n",
        "{\"name\": \"items\", \"auth\": \"oauth2-proxy\", \"public\": false}\n",
        "\"auth\": \"basic-auth\",\n",
        "\"auth\": \"session-cookie\",\n",
    ] {
        assert_miss("docker_config_auth", text);
    }
}

/// A `.pgpass` line has a host (no file name or path) and a password without blanks and with
/// at most one unescaped `:`; `grep -n` output, IPv6 addresses and passwd lines are none.
#[test]
fn pgpass_lines_are_not_grep_output() {
    for (text, secret) in [
        (
            "db.internal.example:5432:shop:shop_app:Quartz-Lantern-82\n",
            "Quartz-Lantern-82",
        ),
        ("*:*:*:replicator:Nimbus\\:Pike-57\n", "Nimbus\\:Pike-57"),
        (
            "pg-primary.svc.cluster.local:5432:*:app:Hollow:Fern-73\n",
            "Hollow:Fern-73",
        ),
    ] {
        assert_hit("pgpass_line", text, secret);
    }
    for text in [
        "app.log:1532:10:15:30 INFO scheduler started\n",
        "values.yaml:12:image:nginx:1.25.3\n",
        "deploy/values.yaml:10:3:image:nginx:1.25.3-alpine\n",
        "fe80:0:0:0:202:b3ff:fe1e:8329\n",
        "nobody:*:-2:-2:Unprivileged User:/var/empty:/usr/bin/false\n",
        "src/main.rs:120:4:error:expected one of `,` or `)`\n",
    ] {
        assert_miss("pgpass_line", text);
    }
}
