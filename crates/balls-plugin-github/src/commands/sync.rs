use crate::config::PluginConfig;
use crate::pr_api::{get_pr, ForgeTaskExt};
use crate::USER_AGENT;
use balls_github_shared::auth;
use balls_github_shared::error::{PluginError, Result};
use balls_github_shared::http::GithubClient;
use balls_github_shared::types::{SyncReport, SyncUpdate, Task};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

pub fn run(task_filter: Option<&str>, config_path: &Path, auth_dir: &Path) -> Result<()> {
    let config = PluginConfig::load(config_path)?;
    let token = auth::load_token(auth_dir)?;
    let client = GithubClient::new(config.api_base(), &token, USER_AGENT);

    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let tasks: Vec<Task> = serde_json::from_str(&buf)?;

    let report = build_report(&client, &config, &tasks, task_filter)?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

/// For every deferred-mode task in `review` with a recorded PR, poll it.
/// When a PR has merged, emit an `updated` entry that closes the task's
/// gate child, carrying the merge SHA in `add_note`. Core's sync-report
/// processing closes the gate child, which unblocks the parent's `bl close`.
pub fn build_report(
    client: &GithubClient,
    config: &PluginConfig,
    tasks: &[Task],
    filter: Option<&str>,
) -> Result<SyncReport> {
    let (owner, name) = config
        .owner_name()
        .ok_or_else(|| PluginError::Config("repo is not owner/name".into()))?;

    let mut report = SyncReport::default();
    for task in tasks {
        if task.status != "review" {
            continue;
        }
        let Some(number) = task.pr_number() else {
            continue;
        };
        if let Some(f) = filter {
            if f != task.id && f != number.to_string() {
                continue;
            }
        }
        let pr = get_pr(client, owner, name, number)?;
        if !pr.merged {
            continue;
        }
        let Some(gate_id) = task.gate_child_id() else {
            continue;
        };
        // Skip if the gate child is gone or already closed (idempotent sync).
        let Some(gate) = tasks.iter().find(|t| t.id == gate_id) else {
            continue;
        };
        if gate.status == "closed" {
            continue;
        }

        let mut fields = BTreeMap::new();
        fields.insert("status".to_string(), Value::String("closed".to_string()));
        let sha = pr.merge_commit_sha.clone().unwrap_or_default();
        report.updated.push(SyncUpdate {
            task_id: gate_id.to_string(),
            fields,
            external: serde_json::Map::new(),
            add_note: format!("PR #{} merged as {}", number, sha),
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "balls-plugin-github-test";

    fn cfg(api: &str) -> PluginConfig {
        serde_json::from_str(&format!(r#"{{"repo":"o/n","api_base":{:?}}}"#, api)).unwrap()
    }

    fn tasks(json: &str) -> Vec<Task> {
        serde_json::from_str(json).unwrap()
    }

    const PARENT: &str = r#"{"id":"bl-p","title":"t","status":"review",
        "links":[{"link_type":"gates","target":"bl-g"}],
        "external":{"github":{"pull_request":{"number":7}}}}"#;

    fn merged_mock(s: &mut mockito::ServerGuard) {
        s.mock("GET", "/repos/o/n/pulls/7")
            .with_status(200)
            .with_body(
                r#"{"number":7,"html_url":"u","head":{"ref":"h","sha":"z"},
                    "base":{"ref":"main"},"merged":true,
                    "merge_commit_sha":"cafe"}"#,
            )
            .create();
    }

    #[test]
    fn rejects_bad_repo() {
        let c = GithubClient::new("http://x", "t", UA);
        let conf: PluginConfig = serde_json::from_str(r#"{"repo":"noslash"}"#).unwrap();
        assert!(build_report(&c, &conf, &[], None).is_err());
    }

    #[test]
    fn skips_uninteresting_tasks() {
        let c = GithubClient::new("http://x", "t", UA);
        let ts = tasks(
            r#"[{"id":"a","title":"t","status":"open"},
                {"id":"b","title":"t","status":"review"}]"#,
        );
        assert!(build_report(&c, &cfg("http://x"), &ts, None)
            .unwrap()
            .updated
            .is_empty());
    }

    #[test]
    fn emits_close_for_merged_pr() {
        let mut s = mockito::Server::new();
        merged_mock(&mut s);
        let c = GithubClient::new(&s.url(), "t", UA);
        let ts = tasks_with_gate();
        let r = build_report(&c, &cfg(&s.url()), &ts, None).unwrap();
        assert_eq!(r.updated.len(), 1);
        assert_eq!(r.updated[0].task_id, "bl-g");
        assert_eq!(r.updated[0].fields["status"], Value::String("closed".into()));
        assert!(r.updated[0].add_note.contains("cafe"));
    }

    fn tasks_with_gate() -> Vec<Task> {
        tasks(&format!(
            r#"[{},{{"id":"bl-g","title":"gate","status":"open"}}]"#,
            PARENT
        ))
    }

    #[test]
    fn not_merged_yields_nothing() {
        let mut s = mockito::Server::new();
        s.mock("GET", "/repos/o/n/pulls/7")
            .with_status(200)
            .with_body(
                r#"{"number":7,"html_url":"u","head":{"ref":"h","sha":"z"},
                    "base":{"ref":"main"},"merged":false}"#,
            )
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let ts = tasks(&format!(r#"[{}]"#, PARENT));
        assert!(build_report(&c, &cfg(&s.url()), &ts, None)
            .unwrap()
            .updated
            .is_empty());
    }

    #[test]
    fn no_gate_or_already_closed_is_skipped() {
        let mut s = mockito::Server::new();
        merged_mock(&mut s);
        let c = GithubClient::new(&s.url(), "t", UA);

        // gate child missing from the task list
        let only_parent = tasks(&format!(r#"[{}]"#, PARENT));
        assert!(build_report(&c, &cfg(&s.url()), &only_parent, None)
            .unwrap()
            .updated
            .is_empty());

        // gate child present but already closed
        let mut s2 = mockito::Server::new();
        merged_mock(&mut s2);
        let c2 = GithubClient::new(&s2.url(), "t", UA);
        let closed_gate = tasks(&format!(
            r#"[{},{{"id":"bl-g","title":"g","status":"closed"}}]"#,
            PARENT
        ));
        assert!(build_report(&c2, &cfg(&s2.url()), &closed_gate, None)
            .unwrap()
            .updated
            .is_empty());

        // parent with PR recorded but no gates link
        let mut s3 = mockito::Server::new();
        merged_mock(&mut s3);
        let c3 = GithubClient::new(&s3.url(), "t", UA);
        let no_link = tasks(
            r#"[{"id":"bl-p","title":"t","status":"review",
                 "external":{"github":{"pull_request":{"number":7}}}}]"#,
        );
        assert!(build_report(&c3, &cfg(&s3.url()), &no_link, None)
            .unwrap()
            .updated
            .is_empty());
    }

    #[test]
    fn filter_matches_id_or_pr_number() {
        let mut s = mockito::Server::new();
        merged_mock(&mut s);
        let c = GithubClient::new(&s.url(), "t", UA);
        let ts = tasks_with_gate();
        // non-matching filter -> skipped before any HTTP
        assert!(build_report(&c, &cfg(&s.url()), &ts, Some("other"))
            .unwrap()
            .updated
            .is_empty());
        // match by local id
        assert_eq!(
            build_report(&c, &cfg(&s.url()), &ts, Some("bl-p"))
                .unwrap()
                .updated
                .len(),
            1
        );
        // match by PR number
        let mut s2 = mockito::Server::new();
        merged_mock(&mut s2);
        let c2 = GithubClient::new(&s2.url(), "t", UA);
        assert_eq!(
            build_report(&c2, &cfg(&s2.url()), &ts, Some("7"))
                .unwrap()
                .updated
                .len(),
            1
        );
    }
}
