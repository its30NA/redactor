//! AI providers.

use super::{check, Family, Rule};

fn r(
    id: &'static str,
    label: &'static str,
    shape: &'static str,
    anchors: &'static [&'static str],
    re: &'static str,
    conf: f32,
) -> Rule {
    Rule::new(Family::Ai, id, label, shape, anchors, re, conf)
}

pub(super) fn rules() -> Vec<Rule> {
    vec![
        r(
            "openai_project_key",
            "OpenAI API Key",
            "sk-proj- + 40+ [A-Za-z0-9_-]",
            &["sk-proj-"],
            r"sk-proj-[A-Za-z0-9_-]{40,}",
            0.99,
        )
        .check(check::random),
        r(
            "openai_service_account_key",
            "OpenAI API Key",
            "sk-svcacct- + 40+ [A-Za-z0-9_-]",
            &["sk-svcacct-"],
            r"sk-svcacct-[A-Za-z0-9_-]{40,}",
            0.99,
        )
        .check(check::random),
        r(
            "openai_admin_key",
            "OpenAI Admin Key",
            "sk-admin- + 40+ [A-Za-z0-9_-]",
            &["sk-admin-"],
            r"sk-admin-[A-Za-z0-9_-]{40,}",
            0.99,
        )
        .check(check::random),
        r(
            "openai_legacy_key",
            "OpenAI API Key",
            "sk- + 20 + T3BlbkFJ + 20",
            &["T3BlbkFJ"],
            r"sk-(?:proj-)?[A-Za-z0-9_-]{20,74}T3BlbkFJ[A-Za-z0-9_-]{20,74}",
            0.99,
        )
        .back(90)
        .fwd(90),
        r(
            "openai_api_key",
            "OpenAI API Key",
            "sk- + 32+ [A-Za-z0-9_-] (generic)",
            &["sk-"],
            r"sk-[A-Za-z0-9_-]{32,}",
            0.85,
        )
        .check(check::random),
        r(
            "anthropic_api_key",
            "Anthropic API Key",
            "sk-ant-api03- + 80+ [A-Za-z0-9_-]",
            &["sk-ant-api03-"],
            r"sk-ant-api03-[A-Za-z0-9_-]{80,}",
            0.99,
        ),
        r(
            "anthropic_admin_key",
            "Anthropic Admin Key",
            "sk-ant-admin01- + 80+ [A-Za-z0-9_-]",
            &["sk-ant-admin01-"],
            r"sk-ant-admin01-[A-Za-z0-9_-]{80,}",
            0.99,
        ),
        r(
            "anthropic_key",
            "Anthropic API Key",
            "sk-ant- + 32+ [A-Za-z0-9_-]",
            &["sk-ant-"],
            r"sk-ant-[A-Za-z0-9_-]{32,}",
            0.95,
        )
        .check(check::random),
        r(
            "huggingface_token",
            "Hugging Face Token",
            "hf_ + 34 [A-Za-z0-9]",
            &["hf_"],
            r"hf_[A-Za-z0-9]{34}",
            0.95,
        )
        .check(check::random),
        r(
            "groq_api_key",
            "Groq API Key",
            "gsk_ + 52 [A-Za-z0-9]",
            &["gsk_"],
            r"gsk_[A-Za-z0-9]{52}",
            0.99,
        ),
        r(
            "replicate_api_token",
            "Replicate API Token",
            "r8_ + 37 [A-Za-z0-9]",
            &["r8_"],
            r"r8_[A-Za-z0-9]{37}",
            0.97,
        )
        .check(check::random),
        r(
            "perplexity_api_key",
            "Perplexity API Key",
            "pplx- + 48 [A-Za-z0-9]",
            &["pplx-"],
            r"pplx-[A-Za-z0-9]{48}",
            0.98,
        ),
        r(
            "xai_api_key",
            "xAI API Key",
            "xai- + 80 [A-Za-z0-9]",
            &["xai-"],
            r"xai-[A-Za-z0-9]{80}",
            0.98,
        ),
        r(
            "openrouter_api_key",
            "OpenRouter API Key",
            "sk-or-v1- + 64 hex",
            &["sk-or-v1-"],
            r"sk-or-v1-[0-9a-f]{64}",
            0.99,
        ),
        r(
            "langsmith_api_key",
            "LangSmith API Key",
            "lsv2_pt_/lsv2_sk_ + 32 hex + _ + 10 hex",
            &["lsv2_pt_", "lsv2_sk_"],
            r"lsv2_(?:pt|sk)_[0-9a-f]{32}_[0-9a-f]{10}",
            0.99,
        ),
        r(
            "pinecone_api_key",
            "Pinecone API Key",
            "pcsk_ + id + _ + 40+ [A-Za-z0-9]",
            &["pcsk_"],
            r"pcsk_[A-Za-z0-9]{4,10}_[A-Za-z0-9]{40,80}",
            0.97,
        ),
        r(
            "tavily_api_key",
            "Tavily API Key",
            "tvly-[dev-|prod-] + 32 [A-Za-z0-9]",
            &["tvly-"],
            r"tvly-(?:dev-|prod-)?[A-Za-z0-9]{32}",
            0.98,
        ),
        r(
            "fireworks_api_key",
            "Fireworks API Key",
            "fw_ + 20-40 [A-Za-z0-9]",
            &["fw_"],
            r"fw_[A-Za-z0-9]{20,40}",
            0.9,
        )
        .check(check::random),
    ]
}
