//! Overlap resolution: merge, never drop.

use crate::finding::{Candidate, Source};

/// Which kind of evidence names a merged span. A vendor pattern knows the format (`OpenAI API
/// Key`), the context engine only the key next to the value (`API Key`), the classifier only
/// that it looks random; personal data is the weakest name for a span that is also a secret.
fn rank(c: &Candidate) -> u8 {
    match c.source {
        Source::Pattern if CONTAINERS.contains(&&*c.detector) => 3,
        // Decoded text sits inside a container (`Basic …`, `"auth": "…"`).
        Source::Decoded => 3,
        Source::Manual | Source::Watchlist | Source::Custom | Source::Pattern => 4,
        Source::Context | Source::Propagated => 2,
        Source::Classifier => 1,
        Source::Pii => 0,
    }
}

/// Whether `a` names a value before `b`: the kind of evidence first, then confidence. The
/// same order picks the label of a merged span here and the one label of a value that
/// several findings name differently (see the engine's render step).
pub(crate) fn outranks(a: &Candidate, b: &Candidate) -> bool {
    let (ra, rb) = (rank(a), rank(b));
    ra > rb || ra == rb && a.confidence > b.confidence
}

/// Patterns that find a secret by the header, URL field or file entry that carries it, not by
/// its shape. A shape pattern on the same value (a JWT after `Bearer`) names it instead.
const CONTAINERS: &[&str] = &[
    "auth_bearer",
    "auth_basic",
    "auth_token_scheme",
    "auth_digest_response",
    "auth_aws4_signature",
    "auth_ntlm",
    "bearer_token",
    "api_key_header",
    "auth_token_header",
    "cookie_header",
    "set_cookie_header",
    "session_id",
    "url_userinfo_password",
    "url_query_token",
    "url_query_client_secret",
    "url_query_key",
    "url_query_password",
    "url_query_signature",
    "connection_string_password",
    "pgpass_line",
    "docker_config_auth",
    "npmrc_auth",
];

/// Merges overlapping candidates into their union, sorted by start.
///
/// Dropping the loser of an overlap can leak: if the winner is shorter, the uncovered rest of
/// the loser (say the first characters of a password) would stay in the output. So every
/// cluster of overlapping spans becomes one span covering their union. Its label, kind and
/// detector come from the highest-confidence member, ties broken by length, then by order.
/// A vendor pattern names the span over a context or classifier finding whatever their
/// confidence, and a token shape over the header or field that carries it (see [`rank`]).
/// Spans that only touch (`a.end == b.start`) stay separate. Empty spans are dropped.
pub fn resolve(mut cands: Vec<Candidate>) -> Vec<Candidate> {
    cands.retain(|c| c.end > c.start);
    // Stable sort: equal spans keep detector order, which keeps the result deterministic.
    cands.sort_by_key(|c| (c.start, c.end));
    // (representative, representative's own length before extension)
    let mut out: Vec<(Candidate, usize)> = Vec::with_capacity(cands.len());
    let mut cluster_end = 0usize;
    for c in cands {
        match out.last_mut() {
            Some((rep, rep_len)) if c.start < cluster_end => {
                cluster_end = cluster_end.max(c.end);
                let start = rep.start;
                let (rc, rr) = (rank(&c), rank(rep));
                let wins = rc > rr
                    || rc == rr
                        && (c.confidence > rep.confidence
                            || (c.confidence == rep.confidence && c.len() > *rep_len));
                if wins {
                    *rep_len = c.len();
                    *rep = c;
                }
                rep.start = start;
                rep.end = cluster_end;
            }
            _ => {
                cluster_end = c.end;
                let len = c.len();
                out.push((c, len));
            }
        }
    }
    out.into_iter().map(|(c, _)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(start: usize, end: usize, label: &'static str, conf: f32) -> Candidate {
        Candidate {
            start,
            end,
            kind: label.into(),
            label: label.into(),
            confidence: conf,
            detector: label.into(),
            reason: "".into(),
            source: Source::Pattern,
        }
    }

    #[test]
    fn merge_never_drops_overlap() {
        let out = resolve(vec![
            cand(20, 40, "Password", 0.9),
            cand(10, 30, "Email", 0.6),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].start, out[0].end), (10, 40));
        assert_eq!(out[0].label, "Password");
    }

    #[test]
    fn ties_broken_by_length() {
        let out = resolve(vec![cand(0, 5, "Short", 0.8), cand(3, 20, "Long", 0.8)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "Long");
        assert_eq!((out[0].start, out[0].end), (0, 20));
    }

    #[test]
    fn chains_and_touching_spans() {
        let out = resolve(vec![
            cand(0, 4, "A", 0.5),
            cand(3, 8, "B", 0.6),
            cand(7, 12, "C", 0.7),
            cand(12, 15, "D", 0.9),
            cand(20, 20, "Empty", 1.0),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].start, out[0].end, &*out[0].label), (0, 12, "C"));
        assert_eq!((out[1].start, out[1].end, &*out[1].label), (12, 15, "D"));
    }

    /// A token shape (JWT) names the span over the header or URL field that carries it.
    #[test]
    fn token_shape_beats_container() {
        let mut bearer = cand(15, 60, "Bearer Token", 0.9);
        bearer.detector = "auth_bearer".into();
        let mut jwt = cand(22, 60, "JSON Web Token", 0.85);
        jwt.detector = "jwt".into();
        let out = resolve(vec![bearer, jwt]);
        assert_eq!(out.len(), 1);
        assert_eq!(
            (out[0].start, out[0].end, &*out[0].label),
            (15, 60, "JSON Web Token")
        );
    }
}
