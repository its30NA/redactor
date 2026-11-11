//! Code hosting, registries, CI and infrastructure tooling.

use super::{check, validate, Family, Hit, Rule, Verdict};

fn r(
    id: &'static str,
    label: &'static str,
    shape: &'static str,
    anchors: &'static [&'static str],
    re: &'static str,
    conf: f32,
) -> Rule {
    Rule::new(Family::Dev, id, label, shape, anchors, re, conf)
}

/// A valid CRC32 checksum makes a GitHub token certain; without it the shape still counts.
fn github_crc(h: &Hit) -> Verdict {
    let s = h.secret();
    if validate::github_checksum_ok(&s[4..]) {
        Verdict::Conf(0.99, "GitHub token format, checksum valid")
    } else {
        Verdict::Conf(0.9, "GitHub token format, checksum not valid")
    }
}

fn github(
    id: &'static str,
    label: &'static str,
    prefix: &'static [&'static str],
    re: &'static str,
) -> Rule {
    r(
        id,
        label,
        "gh?_ + 36 base62 (CRC32 checksum)",
        prefix,
        re,
        0.9,
    )
    .check(github_crc)
}

pub(super) fn rules() -> Vec<Rule> {
    vec![
        r(
            "databricks_token",
            "Databricks Token",
            "dapi + 32 hex (+ -n)",
            &["dapi"],
            r"dapi[a-f0-9]{32}(?:-[0-9]{1,2})?",
            0.95,
        )
        .check(check::random),
        github(
            "github_pat",
            "GitHub Token",
            &["ghp_"],
            r"ghp_[A-Za-z0-9]{36}",
        ),
        github(
            "github_oauth_token",
            "GitHub OAuth Token",
            &["gho_"],
            r"gho_[A-Za-z0-9]{36}",
        ),
        github(
            "github_user_token",
            "GitHub App User Token",
            &["ghu_"],
            r"ghu_[A-Za-z0-9]{36}",
        ),
        github(
            "github_server_token",
            "GitHub App Server Token",
            &["ghs_"],
            r"ghs_[A-Za-z0-9]{36}",
        ),
        github(
            "github_refresh_token",
            "GitHub Refresh Token",
            &["ghr_"],
            r"ghr_[A-Za-z0-9]{36}",
        ),
        r(
            "github_fine_grained_pat",
            "GitHub Token",
            "github_pat_ + 22 + _ + 59 base62",
            &["github_pat_"],
            r"github_pat_[A-Za-z0-9]{22}_[A-Za-z0-9]{59}",
            0.99,
        ),
        r(
            "gitlab_pat",
            "GitLab Token",
            "glpat- + 20+ [A-Za-z0-9_-]",
            &["glpat-"],
            r"glpat-[A-Za-z0-9_-]{20,}(?:\.[A-Za-z0-9_-]+){0,3}",
            0.98,
        ),
        r(
            "gitlab_deploy_token",
            "GitLab Deploy Token",
            "gldt- + 20+ [A-Za-z0-9_-]",
            &["gldt-"],
            r"gldt-[A-Za-z0-9_-]{20,50}",
            0.98,
        ),
        r(
            "gitlab_runner_token",
            "GitLab Runner Token",
            "glrt- + 20+ [A-Za-z0-9_-]",
            &["glrt-"],
            r"glrt-[A-Za-z0-9_-]{20,}(?:\.[A-Za-z0-9_-]+){0,3}",
            0.98,
        ),
        r(
            "gitlab_pipeline_trigger_token",
            "GitLab Pipeline Trigger Token",
            "glptt- + 40 hex",
            &["glptt-"],
            r"glptt-[0-9a-f]{40}",
            0.98,
        ),
        r(
            "gitlab_scim_token",
            "GitLab SCIM Token",
            "glsoat- + 20+ [A-Za-z0-9_-]",
            &["glsoat-"],
            r"glsoat-[A-Za-z0-9_-]{20,50}",
            0.98,
        ),
        r(
            "gitlab_ci_build_token",
            "GitLab CI Job Token",
            "glcbt- + id + _ + 20+ [A-Za-z0-9_-]",
            &["glcbt-"],
            r"glcbt-[0-9a-zA-Z]{1,5}_[0-9a-zA-Z_-]{20,50}",
            0.98,
        ),
        r(
            "gitlab_runner_registration_token",
            "GitLab Runner Registration Token",
            "GR1348941 + 20+ [A-Za-z0-9_-]",
            &["GR1348941"],
            r"GR1348941[A-Za-z0-9_-]{20,50}",
            0.98,
        ),
        r(
            "bitbucket_app_password",
            "Bitbucket App Password",
            "ATBB + 32+ [A-Za-z0-9]",
            &["ATBB"],
            r"ATBB[A-Za-z0-9]{32,64}",
            0.95,
        )
        .check(check::random),
        r(
            "atlassian_api_token",
            "Atlassian API Token",
            "ATATT3 + 150+ [A-Za-z0-9_=-]",
            &["ATATT3"],
            r"ATATT3[A-Za-z0-9_=-]{150,}",
            0.97,
        ),
        r(
            "npm_token",
            "npm Access Token",
            "npm_ + 36 [A-Za-z0-9]",
            &["npm_"],
            r"npm_[A-Za-z0-9]{36}",
            0.98,
        )
        .check(check::random),
        r(
            "pypi_token",
            "PyPI API Token",
            "pypi-Ag + 60+ base64url (macaroon)",
            &["pypi-Ag"],
            r"pypi-Ag[A-Za-z0-9_-]{60,}",
            0.98,
        )
        .fwd(1024),
        r(
            "rubygems_api_key",
            "RubyGems API Key",
            "rubygems_ + 48 hex",
            &["rubygems_"],
            r"rubygems_[0-9a-f]{48}",
            0.99,
        ),
        r(
            "nuget_api_key",
            "NuGet API Key",
            "oy2 + 43 [a-z0-9]",
            &["oy2"],
            r"oy2[a-z0-9]{43}",
            0.9,
        )
        .check(check::random),
        r(
            "dockerhub_pat",
            "Docker Hub Token",
            "dckr_pat_ + 20-40 [A-Za-z0-9_-]",
            &["dckr_pat_"],
            r"dckr_pat_[A-Za-z0-9_-]{20,40}",
            0.99,
        ),
        r(
            "terraform_cloud_token",
            "Terraform Cloud Token",
            "14 [A-Za-z0-9] + .atlasv1. + 60-70 [A-Za-z0-9_=-]",
            &[".atlasv1."],
            r"[A-Za-z0-9]{14}\.atlasv1\.[A-Za-z0-9_=-]{60,}",
            0.99,
        )
        .back(14)
        .fwd(90),
        r(
            "vault_service_token",
            "Vault Token",
            "hvs. + 24+ [A-Za-z0-9_-]",
            &["hvs."],
            r"hvs\.[A-Za-z0-9_-]{24,}",
            0.97,
        )
        .check(check::random),
        r(
            "vault_batch_token",
            "Vault Batch Token",
            "hvb. + 24+ [A-Za-z0-9_-]",
            &["hvb."],
            r"hvb\.[A-Za-z0-9_-]{24,}",
            0.97,
        )
        .check(check::random),
        r(
            "vault_recovery_token",
            "Vault Recovery Token",
            "hvr. + 24+ [A-Za-z0-9_-]",
            &["hvr."],
            r"hvr\.[A-Za-z0-9_-]{24,}",
            0.97,
        )
        .check(check::random),
        r(
            "doppler_token",
            "Doppler Token",
            "dp.pt./dp.st./dp.sa./dp.ct. + 40-44 [A-Za-z0-9]",
            &["dp.pt.", "dp.st.", "dp.sa.", "dp.ct."],
            r"dp\.(?:pt|st|sa|ct)\.(?:[a-z0-9_-]{2,35}\.)?[A-Za-z0-9]{40,}",
            0.98,
        ),
        r(
            "pulumi_token",
            "Pulumi Access Token",
            "pul- + 40 hex",
            &["pul-"],
            r"pul-[a-f0-9]{40}",
            0.98,
        ),
        r(
            "netlify_token",
            "Netlify Token",
            "nfp_ + 36 [A-Za-z0-9]",
            &["nfp_"],
            r"nfp_[A-Za-z0-9]{36}",
            0.98,
        ),
        r(
            "heroku_api_key",
            "Heroku API Key",
            "HRKU- + 30-80 [A-Za-z0-9_-]",
            &["HRKU-"],
            r"HRKU-[A-Za-z0-9_-]{30,80}",
            0.98,
        ),
        r(
            "digitalocean_pat",
            "DigitalOcean Token",
            "dop_v1_ + 64 hex",
            &["dop_v1_"],
            r"dop_v1_[a-f0-9]{64}",
            0.99,
        ),
        r(
            "digitalocean_oauth_token",
            "DigitalOcean OAuth Token",
            "doo_v1_ + 64 hex",
            &["doo_v1_"],
            r"doo_v1_[a-f0-9]{64}",
            0.99,
        ),
        r(
            "digitalocean_refresh_token",
            "DigitalOcean Refresh Token",
            "dor_v1_ + 64 hex",
            &["dor_v1_"],
            r"dor_v1_[a-f0-9]{64}",
            0.99,
        ),
        r(
            "supabase_pat",
            "Supabase Token",
            "sbp_ + 40 hex",
            &["sbp_"],
            r"sbp_[a-f0-9]{40}",
            0.98,
        ),
        r(
            "supabase_secret_key",
            "Supabase Secret Key",
            "sb_secret_ + 20+ [A-Za-z0-9_-]",
            &["sb_secret_"],
            r"sb_secret_[A-Za-z0-9_-]{20,64}",
            0.98,
        ),
        r(
            "planetscale_token",
            "PlanetScale Token",
            "pscale_tkn_ + 32-64 [A-Za-z0-9_-]",
            &["pscale_tkn_"],
            r"pscale_tkn_[A-Za-z0-9_-]{32,64}",
            0.98,
        ),
        r(
            "planetscale_password",
            "PlanetScale Password",
            "pscale_pw_ + 32-64 [A-Za-z0-9_-]",
            &["pscale_pw_"],
            r"pscale_pw_[A-Za-z0-9_-]{32,64}",
            0.98,
        ),
        r(
            "planetscale_oauth_token",
            "PlanetScale OAuth Token",
            "pscale_oauth_ + 32-64 [A-Za-z0-9_-]",
            &["pscale_oauth_"],
            r"pscale_oauth_[A-Za-z0-9_-]{32,64}",
            0.98,
        ),
        r(
            "postman_api_key",
            "Postman API Key",
            "PMAK- + 24 hex + - + 34 hex",
            &["PMAK-"],
            r"PMAK-[a-f0-9]{24}-[a-f0-9]{34}",
            0.99,
        ),
        r(
            "sentry_org_token",
            "Sentry Token",
            "sntrys_eyJ + 50+ base64",
            &["sntrys_eyJ"],
            r"sntrys_eyJ[A-Za-z0-9_=+/-]{50,}",
            0.99,
        )
        .fwd(1024),
        r(
            "sentry_user_token",
            "Sentry Token",
            "sntryu_ + 64 hex",
            &["sntryu_"],
            r"sntryu_[a-f0-9]{64}",
            0.99,
        ),
        r(
            "grafana_service_account_token",
            "Grafana Token",
            "glsa_ + 32 [A-Za-z0-9] + _ + 8 hex",
            &["glsa_"],
            r"glsa_[A-Za-z0-9]{32}_[A-Fa-f0-9]{8}",
            0.99,
        ),
        r(
            "grafana_cloud_token",
            "Grafana Cloud Token",
            "glc_ + 32+ base64",
            &["glc_"],
            r"glc_[A-Za-z0-9+/]{32,}={0,2}",
            0.97,
        )
        .check(check::random),
        r(
            "newrelic_user_key",
            "New Relic API Key",
            "NRAK- + 27 [A-Z0-9]",
            &["NRAK-"],
            r"NRAK-[A-Z0-9]{27}",
            0.98,
        ),
        r(
            "sonarqube_token",
            "SonarQube Token",
            "squ_/sqp_/sqa_ + 40 hex",
            &["squ_", "sqp_", "sqa_"],
            r"sq[upa]_[a-f0-9]{40}",
            0.97,
        ),
        r(
            "circleci_pat",
            "CircleCI Token",
            "CCIPAT_ + 22 + _ + 40 hex",
            &["CCIPAT_"],
            r"CCIPAT_[A-Za-z0-9]{22}_[a-f0-9]{40}",
            0.99,
        ),
        r(
            "buildkite_token",
            "Buildkite Token",
            "bkua_ + 40 hex",
            &["bkua_"],
            r"bkua_[a-f0-9]{40}",
            0.99,
        ),
        r(
            "linear_api_key",
            "Linear API Key",
            "lin_api_ + 40 [A-Za-z0-9]",
            &["lin_api_"],
            r"lin_api_[A-Za-z0-9]{40}",
            0.99,
        ),
        r(
            "linear_oauth_token",
            "Linear OAuth Token",
            "lin_oauth_ + 64 hex",
            &["lin_oauth_"],
            r"lin_oauth_[a-f0-9]{64}",
            0.99,
        ),
    ]
}
