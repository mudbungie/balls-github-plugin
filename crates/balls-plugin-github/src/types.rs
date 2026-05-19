//! Forge plugin's `external.github.*` projection.
//!
//! The shape stored under `task.external.github`: a `pull_request`
//! ref. Core stores this verbatim after push; on the next cycle the
//! plugin reads it back via `pr_api::ForgeTaskExt::pr_number`.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct PrRef {
    pub number: u64,
    pub url: String,
    pub head_sha: String,
    pub target_branch: String,
}

#[derive(Debug, Serialize)]
pub struct PushResponse {
    pub pull_request: PrRef,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_push_response() {
        let r = PushResponse {
            pull_request: PrRef {
                number: 3,
                url: "https://gh/pr/3".into(),
                head_sha: "abc".into(),
                target_branch: "main".into(),
            },
        };
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains(r#""pull_request":{"number":3"#));
        assert!(s.contains(r#""target_branch":"main""#));
    }
}
