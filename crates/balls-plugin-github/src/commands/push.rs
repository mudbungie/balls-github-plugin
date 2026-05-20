use crate::config::PluginConfig;
use crate::pr_api::{create_pr, find_pr, get_pr, ForgeTaskExt};
use crate::types::PushResponse;
use crate::USER_AGENT;
use balls_github_shared::auth;
use balls_github_shared::error::{PluginError, Result};
use balls_github_shared::http::GithubClient;
use balls_github_shared::types::Task;
use std::io::Read;
use std::path::Path;

pub fn run(task_id: &str, config_path: &Path, auth_dir: &Path) -> Result<()> {
    let config = PluginConfig::load(config_path)?;
    let token = auth::load_token(auth_dir)?;
    let client = GithubClient::new(config.api_base(), &token, USER_AGENT);

    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    let task: Task = serde_json::from_str(&buf)?;
    if task.id != task_id {
        return Err(PluginError::Other(format!(
            "--task {} does not match stdin task id {}",
            task_id, task.id
        )));
    }

    let resp = push_task(&client, &config, &task)?;
    println!("{}", serde_json::to_string(&resp)?);
    Ok(())
}

/// Open or update the PR for a task that `bl review` pushed in deferred mode.
/// Idempotent: an existing PR (by stored number, or by head branch) is reused
/// rather than duplicated.
pub fn push_task(
    client: &GithubClient,
    config: &PluginConfig,
    task: &Task,
) -> Result<PushResponse> {
    if task.status != "review" {
        return Err(PluginError::Other(format!(
            "task {} is {}, not review; nothing to push",
            task.id, task.status
        )));
    }
    let (owner, name) = config
        .owner_name()
        .ok_or_else(|| PluginError::Config("repo is not owner/name".into()))?;
    let base = task
        .target_branch
        .clone()
        .or_else(|| config.target_branch.clone())
        .ok_or_else(|| {
            PluginError::Config(format!(
                "no target_branch for {}; set it per-task or in config \
                 (deferred mode requires an explicit PR base)",
                task.id
            ))
        })?;

    let head = format!("work/{}", task.id);
    let title = format!("{} [{}]", task.title, task.id);
    let body = format!("Delivers {} via balls deferred-mode review.", task.id);

    let pr = match task.pr_number() {
        Some(n) => get_pr(client, owner, name, n)?,
        None => match find_pr(client, owner, name, &head)? {
            Some(existing) => existing,
            None => create_pr(client, owner, name, &title, &head, &base, &body)?,
        },
    };
    Ok(PushResponse {
        pull_request: pr.to_ref(&base),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const UA: &str = "balls-plugin-github-test";

    fn cfg(api: &str, target: Option<&str>) -> PluginConfig {
        let t = target
            .map(|s| format!(r#","target_branch":{:?}"#, s))
            .unwrap_or_default();
        serde_json::from_str(&format!(r#"{{"repo":"o/n","api_base":{:?}{}}}"#, api, t)).unwrap()
    }

    fn task(json: &str) -> Task {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn rejects_non_review_task() {
        let c = GithubClient::new("http://x", "t", UA);
        let err = push_task(
            &c,
            &cfg("http://x", Some("main")),
            &task(r#"{"id":"bl-1","title":"t","status":"in_progress"}"#),
        )
        .unwrap_err();
        assert!(err.to_string().contains("not review"));
    }

    #[test]
    fn rejects_bad_repo() {
        let c = GithubClient::new("http://x", "t", UA);
        let conf: PluginConfig = serde_json::from_str(r#"{"repo":"noslash"}"#).unwrap();
        assert!(push_task(&c, &conf, &task(r#"{"id":"bl-1","title":"t","status":"review"}"#))
            .unwrap_err()
            .to_string()
            .contains("owner/name"));
    }

    #[test]
    fn rejects_missing_target_branch() {
        let c = GithubClient::new("http://x", "t", UA);
        let err = push_task(
            &c,
            &cfg("http://x", None),
            &task(r#"{"id":"bl-1","title":"t","status":"review"}"#),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no target_branch"));
    }

    #[test]
    fn creates_pr_when_none_exists() {
        let mut s = mockito::Server::new();
        s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls\?".into()))
            .with_status(200)
            .with_body("[]")
            .create();
        s.mock("POST", "/repos/o/n/pulls")
            .with_status(201)
            .with_body(
                r#"{"number":5,"html_url":"https://gh/pr/5",
                    "head":{"ref":"work/bl-1","sha":"sha5"},"base":{"ref":"main"}}"#,
            )
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let r = push_task(
            &c,
            &cfg(&s.url(), Some("main")),
            &task(r#"{"id":"bl-1","title":"Do it","status":"review"}"#),
        )
        .unwrap();
        assert_eq!(r.pull_request.number, 5);
        assert_eq!(r.pull_request.head_sha, "sha5");
        assert_eq!(r.pull_request.target_branch, "main");
    }

    #[test]
    fn reuses_existing_pr_by_head() {
        let mut s = mockito::Server::new();
        s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls\?".into()))
            .with_status(200)
            .with_body(
                r#"[{"number":8,"html_url":"u","head":{"ref":"work/bl-2","sha":"s8"},
                     "base":{"ref":"develop"}}]"#,
            )
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let r = push_task(
            &c,
            &cfg(&s.url(), Some("main")),
            &task(r#"{"id":"bl-2","title":"t","status":"review","target_branch":"develop"}"#),
        )
        .unwrap();
        assert_eq!(r.pull_request.number, 8);
        assert_eq!(r.pull_request.target_branch, "develop");
    }

    #[test]
    fn fetches_pr_by_stored_number() {
        let mut s = mockito::Server::new();
        s.mock("GET", "/repos/o/n/pulls/3")
            .with_status(200)
            .with_body(
                r#"{"number":3,"html_url":"u","head":{"ref":"work/bl-3","sha":"s3"},
                    "base":{"ref":"main"}}"#,
            )
            .create();
        let c = GithubClient::new(&s.url(), "t", UA);
        let r = push_task(
            &c,
            &cfg(&s.url(), Some("main")),
            &task(
                r#"{"id":"bl-3","title":"t","status":"review",
                    "external":{"github":{"pull_request":{"number":3}}}}"#,
            ),
        )
        .unwrap();
        assert_eq!(r.pull_request.number, 3);
    }
}
