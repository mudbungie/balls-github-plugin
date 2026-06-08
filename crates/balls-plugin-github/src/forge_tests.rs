use super::*;
use std::cell::RefCell;

/// A fake [`Forge`] recording its calls and returning canned values, so the
/// pure [`dispatch`] matrix is exercised without a repo, network, or `bl`.
#[derive(Default)]
struct Fake {
    recall: Option<String>,
    has_changes: bool,
    pending: Vec<(String, String)>,
    merged_parents: Vec<String>,
    calls: RefCell<Vec<String>>,
}

impl Fake {
    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }
    fn log(&self, s: String) {
        self.calls.borrow_mut().push(s);
    }
}

impl Forge for Fake {
    fn create_gate(&self, parent: &str, _title: &str) -> Result<String> {
        self.log(format!("create_gate {parent}"));
        Ok("bl-gate".into())
    }
    fn remember_gate(&self, parent: &str, gate: &str) -> Result<()> {
        self.log(format!("remember {parent}={gate}"));
        Ok(())
    }
    fn recall_gate(&self, _parent: &str) -> Result<Option<String>> {
        Ok(self.recall.clone())
    }
    fn forget_gate(&self, parent: &str) -> Result<()> {
        self.log(format!("forget {parent}"));
        Ok(())
    }
    fn close_gate(&self, gate: &str) -> Result<()> {
        self.log(format!("close_gate {gate}"));
        Ok(())
    }
    fn drop_gate(&self, gate: &str) -> Result<()> {
        self.log(format!("drop_gate {gate}"));
        Ok(())
    }
    fn capture(&self, id: &str, _title: &str) -> Result<()> {
        self.log(format!("capture {id}"));
        Ok(())
    }
    fn has_changes(&self, _id: &str, _base: &str) -> Result<bool> {
        Ok(self.has_changes)
    }
    fn push_pr(&self, id: &str, _title: &str, base: &str) -> Result<String> {
        self.log(format!("push_pr {id} -> {base}"));
        Ok("https://pr/1".into())
    }
    fn teardown(&self, id: &str) -> Result<()> {
        self.log(format!("teardown {id}"));
        Ok(())
    }
    fn pr_merged(&self, parent: &str) -> Result<bool> {
        Ok(self.merged_parents.iter().any(|p| p == parent))
    }
    fn pending_gates(&self) -> Result<Vec<(String, String)>> {
        Ok(self.pending.clone())
    }
}

fn ctx() -> Ctx {
    Ctx { id: "bl-p".into(), title: "T".into(), base: "main".into() }
}

fn run(op: &str, phase: &str, rb: bool, f: &Fake) -> Option<String> {
    dispatch(op, phase, rb, f, &ctx()).unwrap()
}

#[test]
fn protocol_lists_the_delivery_ops() {
    for op in ["claim", "close", "drop", "sync"] {
        assert!(PROTOCOL_JSON.contains(op));
    }
    assert!(PROTOCOL_JSON.contains(r#""protocol":[1]"#));
}

#[test]
fn claim_post_opens_gate_when_absent() {
    let f = Fake::default();
    assert_eq!(run("claim", "post", false, &f), None);
    assert_eq!(f.calls(), ["create_gate bl-p", "remember bl-p=bl-gate"]);
}

#[test]
fn claim_post_is_idempotent_when_gate_exists() {
    let f = Fake { recall: Some("bl-gate".into()), ..Default::default() };
    assert_eq!(run("claim", "post", false, &f), None);
    assert!(f.calls().is_empty());
}

#[test]
fn claim_post_rollback_drops_the_gate() {
    let f = Fake { recall: Some("bl-gate".into()), ..Default::default() };
    assert_eq!(run("claim", "post", true, &f), None);
    assert_eq!(f.calls(), ["drop_gate bl-gate", "forget bl-p"]);
}

#[test]
fn claim_post_rollback_is_noop_without_a_gate() {
    let f = Fake::default();
    assert_eq!(run("claim", "post", true, &f), None);
    assert!(f.calls().is_empty());
}

#[test]
fn close_pre_pushes_a_pr_when_there_are_changes() {
    let f = Fake { has_changes: true, ..Default::default() };
    assert_eq!(run("close", "pre", false, &f), Some("https://pr/1".into()));
    assert_eq!(f.calls(), ["capture bl-p", "push_pr bl-p -> main"]);
}

#[test]
fn close_pre_auto_resolves_the_gate_when_empty() {
    let f = Fake { recall: Some("bl-gate".into()), ..Default::default() };
    assert_eq!(run("close", "pre", false, &f), None);
    assert_eq!(f.calls(), ["capture bl-p", "close_gate bl-gate", "forget bl-p"]);
}

#[test]
fn close_pre_empty_without_a_gate_just_captures() {
    let f = Fake::default();
    assert_eq!(run("close", "pre", false, &f), None);
    assert_eq!(f.calls(), ["capture bl-p"]);
}

#[test]
fn close_pre_rollback_is_a_noop() {
    let f = Fake { has_changes: true, ..Default::default() };
    assert_eq!(run("close", "pre", true, &f), None);
    assert!(f.calls().is_empty());
}

#[test]
fn drop_post_tears_down_and_drops_the_gate() {
    let f = Fake { recall: Some("bl-gate".into()), ..Default::default() };
    assert_eq!(run("drop", "post", false, &f), None);
    assert_eq!(f.calls(), ["teardown bl-p", "drop_gate bl-gate", "forget bl-p"]);
}

#[test]
fn drop_post_without_a_gate_only_tears_down() {
    let f = Fake::default();
    assert_eq!(run("drop", "post", false, &f), None);
    assert_eq!(f.calls(), ["teardown bl-p"]);
}

#[test]
fn sync_closes_only_merged_gates() {
    let f = Fake {
        pending: vec![("bl-a".into(), "bl-ga".into()), ("bl-b".into(), "bl-gb".into())],
        merged_parents: vec!["bl-a".into()],
        ..Default::default()
    };
    assert_eq!(run("sync", "post", false, &f), None);
    assert_eq!(f.calls(), ["close_gate bl-ga", "forget bl-a"]);
}

#[test]
fn unwired_hook_is_a_noop() {
    let f = Fake::default();
    assert_eq!(run("update", "post", false, &f), None);
    assert_eq!(run("claim", "pre", false, &f), None);
    assert!(f.calls().is_empty());
}
