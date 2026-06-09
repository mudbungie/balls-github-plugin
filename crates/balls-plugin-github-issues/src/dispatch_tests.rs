//! Tests for the §6 process surface: protocol, auth subcommands, and `<op>
//! <phase>` routing (acting slots, import suppression, op/phase gating).

use super::*;
use crate::territory::Xdg;
use std::path::PathBuf;

const UA_USER: &str = "/user";

struct Harness {
    dir: tempfile::TempDir,
    bl_bin: std::ffi::OsString,
}

impl Harness {
    fn new() -> Self {
        Self { dir: tempfile::tempdir().unwrap(), bl_bin: "bl".into() }
    }

    fn xdg(&self) -> Xdg {
        Xdg { home: self.dir.path().join("home"), state_home: Some(self.dir.path().join("state")) }
    }

    fn env<'a>(&'a self, cwd: &'a std::path::Path, importing: bool, api_base: String) -> Env<'a> {
        Env { xdg: self.xdg(), cwd, bl_bin: self.bl_bin.clone(), importing, default_api_base: api_base }
    }

    fn territory(&self, invocation: &str) -> PathBuf {
        self.xdg().territory(invocation)
    }

    /// Write the committed config under a landing dir, returning the landing path.
    fn write_config(&self, api_base: &str) -> PathBuf {
        let landing = self.dir.path().join("landing");
        let dir = landing.join("config").join("plugins").join("github-issues");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), format!(r#"{{"repo":"o/n","api_base":"{api_base}"}}"#))
            .unwrap();
        landing
    }
}

fn run_str(args: &[&str], stdin: &str, env: &Env) -> (i32, String) {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut out = Vec::new();
    let code = run(&owned, &mut stdin.as_bytes(), &mut out, env);
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn protocol_self_describes() {
    let h = Harness::new();
    let cwd = h.dir.path();
    let (code, out) = run_str(&["protocol"], "", &h.env(cwd, false, "x".into()));
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["protocol"], 1);
    let ops = v["ops"].as_array().unwrap();
    assert!(ops.iter().any(|o| o == "sync") && ops.iter().any(|o| o == "close"));
}

#[test]
fn bad_argv_is_a_usage_error() {
    let h = Harness::new();
    let cwd = h.dir.path();
    let (code, _) = run_str(&["wat", "too", "many"], "", &h.env(cwd, false, "x".into()));
    assert_eq!(code, 1);
}

#[test]
fn auth_setup_then_check_round_trip() {
    let h = Harness::new();
    let cwd = h.dir.path();
    let mut s = mockito::Server::new();
    s.mock("GET", UA_USER).with_status(200).with_body(r#"{"login":"octocat"}"#).expect(2).create();

    let env = h.env(cwd, false, s.url());
    let (code, _) = run_str(&["auth-setup"], "ghp_tok\n", &env);
    assert_eq!(code, 0);
    // token landed in territory(cwd)
    let tok = h.territory(cwd.to_str().unwrap()).join("token.json");
    assert!(tok.exists());

    let (code, _) = run_str(&["auth-check"], "", &env);
    assert_eq!(code, 0);
}

#[test]
fn auth_setup_rejects_empty_token() {
    let h = Harness::new();
    let cwd = h.dir.path();
    let (code, _) = run_str(&["auth-setup"], "   \n", &h.env(cwd, false, "x".into()));
    assert_eq!(code, 1);
}

#[test]
fn auth_check_without_a_token_fails() {
    let h = Harness::new();
    let cwd = h.dir.path();
    let (code, _) = run_str(&["auth-check"], "", &h.env(cwd, false, "x".into()));
    assert_eq!(code, 1);
}

#[test]
fn non_acting_slots_noop_without_config() {
    let h = Harness::new();
    let cwd = h.dir.path();
    let env = h.env(cwd, false, "x".into());
    // create/pre and claim/post are not acting slots → exit 0, no config/token needed.
    for (op, phase) in [("create", "pre"), ("claim", "post"), ("sync", "pre")] {
        let payload = format!(r#"{{"op":"{op}","phase":"{phase}","binding":{{"invocation_path":"/p"}}}}"#);
        let (code, _) = run_str(&[op, phase], &payload, &env);
        assert_eq!(code, 0, "{op}/{phase}");
    }
}

#[test]
fn importing_suppresses_a_push_slot() {
    let h = Harness::new();
    let cwd = h.dir.path();
    // importing=true → create/post returns Ok before any config/GitHub work.
    let env = h.env(cwd, true, "x".into());
    let payload =
        r#"{"op":"create","phase":"post","binding":{"invocation_path":"/p"},"metadata":{"bl-id":["bl-1"]}}"#;
    let (code, _) = run_str(&["create", "post"], payload, &env);
    assert_eq!(code, 0);
}

#[test]
fn argv_op_must_match_payload_op() {
    let h = Harness::new();
    let cwd = h.dir.path();
    let env = h.env(cwd, false, "x".into());
    let payload = r#"{"op":"update","phase":"post","binding":{"invocation_path":"/p"}}"#;
    let (code, _) = run_str(&["close", "post"], payload, &env);
    assert_eq!(code, 1);
}

#[test]
fn acting_push_slot_loads_config_and_calls_github() {
    let h = Harness::new();
    let cwd = h.dir.path();
    let mut s = mockito::Server::new();
    // auth probe is not used on the hook path; just the create POST.
    s.mock("POST", "/repos/o/n/issues")
        .with_status(201)
        .with_body(r#"{"number":42,"state":"open"}"#)
        .create();
    let landing = h.write_config(&s.url());
    let invocation = cwd.join("proj");
    std::fs::create_dir_all(&invocation).unwrap();
    // store the token in territory(invocation)
    balls_github_shared::auth::save_token(&h.territory(invocation.to_str().unwrap()), "tok").unwrap();
    // the store checkout push reads the ball's title/body from
    let store = cwd.join("store");
    std::fs::create_dir_all(store.join("tasks")).unwrap();
    std::fs::write(store.join("tasks").join("bl-1a2b.md"), "+++\ntitle = \"Hi\"\ncreated = 1\n+++\nb").unwrap();

    let env = h.env(cwd, false, "x".into());
    let payload = format!(
        r#"{{"op":"create","phase":"post","actor":"me",
            "binding":{{"landing":"{}","store":"{}","invocation_path":"{}"}},
            "metadata":{{"bl-id":["bl-1a2b"]}}}}"#,
        landing.display(),
        store.display(),
        invocation.display(),
    );
    let (code, _) = run_str(&["create", "post"], &payload, &env);
    assert_eq!(code, 0);
    let base = Base::load(&h.territory(invocation.to_str().unwrap())).unwrap();
    assert_eq!(base.get("bl-1a2b").unwrap().number, 42);
}

#[test]
fn acting_sync_slot_runs_the_pull() {
    let h = Harness::new();
    let cwd = h.dir.path();
    let mut s = mockito::Server::new();
    s.mock("GET", "/repos/o/n/issues?state=all&per_page=100")
        .with_status(200)
        .with_body("[]")
        .create();
    let landing = h.write_config(&s.url());
    let invocation = cwd.join("proj");
    std::fs::create_dir_all(invocation.join("store").join("tasks")).unwrap();
    balls_github_shared::auth::save_token(&h.territory(invocation.to_str().unwrap()), "tok").unwrap();

    let env = h.env(cwd, false, "x".into());
    let payload = format!(
        r#"{{"op":"sync","phase":"post","actor":"me",
            "binding":{{"landing":"{}","store":"{}","invocation_path":"{}"}}}}"#,
        landing.display(),
        invocation.join("store").display(),
        invocation.display(),
    );
    let (code, _) = run_str(&["sync", "post"], &payload, &env);
    assert_eq!(code, 0);
}

#[test]
fn adopt_stamps_the_marker_on_a_legacy_issue() {
    let h = Harness::new();
    let cwd = h.dir.path(); // the cwd keys the territory, like auth-setup
    let legacy = cwd.join("legacy");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(
        legacy.join("bl-1a2b.json"),
        r#"{"id":"bl-1a2b","status":"open","external":{"github-issues":{"issue":{"number":7}}}}"#,
    )
    .unwrap();
    let mut s = mockito::Server::new();
    s.mock("GET", "/repos/o/n/issues/7")
        .with_status(200)
        .with_body(r#"{"number":7,"title":"Old","state":"open"}"#)
        .create();
    s.mock("PATCH", "/repos/o/n/issues/7")
        .with_status(200)
        .with_body(r#"{"number":7,"title":"Old [bl-1a2b]","state":"open"}"#)
        .create();
    // repo + api_base come from the committed config; the token from territory(cwd).
    let landing = h.write_config(&s.url());
    let config = config_path(landing.to_str().unwrap());
    balls_github_shared::auth::save_token(&h.territory(cwd.to_str().unwrap()), "tok").unwrap();

    let env = h.env(cwd, false, "x".into());
    let args = ["adopt", legacy.to_str().unwrap(), config.to_str().unwrap()];
    let (code, _) = run_str(&args, "", &env);
    assert_eq!(code, 0);
}

#[test]
fn adopt_errors_on_a_missing_legacy_dir() {
    let h = Harness::new();
    let cwd = h.dir.path();
    let landing = h.write_config("https://api.github.com");
    let config = config_path(landing.to_str().unwrap());
    balls_github_shared::auth::save_token(&h.territory(cwd.to_str().unwrap()), "tok").unwrap();
    let env = h.env(cwd, false, "x".into());
    let args = ["adopt", "/no/such/dir", config.to_str().unwrap()];
    let (code, _) = run_str(&args, "", &env);
    assert_eq!(code, 1);
}

#[test]
fn acting_slot_errors_when_config_missing() {
    let h = Harness::new();
    let cwd = h.dir.path();
    let env = h.env(cwd, false, "x".into());
    let payload = r#"{"op":"sync","phase":"post","binding":{"landing":"/nope","store":"/s","invocation_path":"/p"}}"#;
    let (code, _) = run_str(&["sync", "post"], payload, &env);
    assert_eq!(code, 1);
}
