use super::*;
use crate::wire::Gate;
use balls_github_shared::error::PluginError;
use std::cell::RefCell;

/// A fake [`Forge`] recording its calls and returning canned values, so the
/// pure [`dispatch`] matrix is exercised without a repo, network, or `bl`.
#[derive(Default)]
struct Fake {
    /// Canned `(gate id, parent)` rows the open-gate scan returns.
    gates: Vec<(&'static str, &'static str)>,
    /// Parents whose PR has merged, with the URL the close note carries.
    merged: Vec<(&'static str, &'static str)>,
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
    fn open_gates(&self) -> Result<Vec<Gate>> {
        Ok(self
            .gates
            .iter()
            .map(|(id, parent)| Gate { id: (*id).into(), parent: (*parent).into() })
            .collect())
    }
    fn mint_gate(&self, parent: &str, title: &str) -> Result<String> {
        self.log(format!("mint {parent} '{title}'"));
        Ok("bl-gate".into())
    }
    fn close_gate(&self, gate: &str, note: &str) -> Result<()> {
        self.log(format!("close {gate}: {note}"));
        Ok(())
    }
    fn merged_pr(&self, parent: &str) -> Result<Option<String>> {
        Ok(self.merged.iter().find(|(p, _)| *p == parent).map(|(_, url)| (*url).to_string()))
    }
}

fn ctx() -> Ctx {
    Ctx { id: "bl-p".into(), title: "T".into(), gate_of: None }
}

fn run(op: &str, phase: &str, rb: bool, f: &Fake, ctx: &Ctx) -> Option<String> {
    dispatch(op, phase, rb, f, ctx).unwrap()
}

#[test]
fn protocol_lists_the_two_hooked_ops() {
    assert!(PROTOCOL_JSON.contains(r#""ops":["claim","sync"]"#));
    assert!(PROTOCOL_JSON.contains(r#""protocol":[1]"#));
}

#[test]
fn claim_post_mints_the_gate_and_prints_its_id() {
    let f = Fake::default();
    assert_eq!(run("claim", "post", false, &f, &ctx()), Some("bl-gate".into()));
    assert_eq!(f.calls(), vec!["mint bl-p 'T'"]);
}

#[test]
fn claim_post_skips_when_the_claimed_task_is_a_gate_child() {
    // No gates-for-gates: the claimed ball carries the plugin's own join key.
    let f = Fake::default();
    let c = Ctx { gate_of: Some("bl-elder".into()), ..ctx() };
    assert_eq!(run("claim", "post", false, &f, &c), None);
    assert!(f.calls().is_empty());
}

#[test]
fn claim_post_reuses_a_standing_open_gate() {
    // Unclaim leaves the gate open (bl-7bfe); a reclaim must not mint a second.
    let f = Fake { gates: vec![("bl-g1", "bl-p")], ..Fake::default() };
    assert_eq!(run("claim", "post", false, &f, &ctx()), None);
    assert!(f.calls().is_empty());
}

#[test]
fn rollback_claim_closes_the_derived_gate() {
    let f = Fake { gates: vec![("bl-g1", "bl-p"), ("bl-g2", "bl-other")], ..Fake::default() };
    assert_eq!(run("claim", "post", true, &f, &ctx()), None);
    assert_eq!(f.calls(), vec!["close bl-g1: review gate withdrawn: the claim rolled back"]);
}

#[test]
fn rollback_claim_without_a_gate_is_a_no_op() {
    // Idempotent (§14): the mint never happened, or was already undone.
    let f = Fake::default();
    assert_eq!(run("claim", "post", true, &f, &ctx()), None);
    assert!(f.calls().is_empty());
}

#[test]
fn sync_closes_merged_gates_and_reports_them() {
    let f = Fake {
        gates: vec![("bl-g1", "bl-p"), ("bl-g2", "bl-q")],
        merged: vec![("bl-p", "https://gh/pr/4")],
        ..Fake::default()
    };
    let out = run("sync", "post", false, &f, &Ctx::default()).unwrap();
    assert_eq!(out, "bl-g1 resolved: bl-p merged (https://gh/pr/4)");
    // bl-q's PR has not merged — its gate stays open.
    assert_eq!(f.calls(), vec!["close bl-g1: PR merged: https://gh/pr/4"]);
}

#[test]
fn sync_with_nothing_merged_is_silent() {
    let f = Fake { gates: vec![("bl-g1", "bl-p")], ..Fake::default() };
    assert_eq!(run("sync", "post", false, &f, &Ctx::default()), None);
    assert!(f.calls().is_empty());
}

#[test]
fn unwired_hooks_no_op() {
    let f = Fake::default();
    for (op, phase, rb) in
        [("close", "pre", false), ("unclaim", "post", false), ("sync", "post", true), ("claim", "pre", false)]
    {
        assert_eq!(run(op, phase, rb, &f, &ctx()), None);
    }
    assert!(f.calls().is_empty());
}

#[test]
fn errors_propagate_from_the_seam() {
    struct Broken;
    impl Forge for Broken {
        fn open_gates(&self) -> Result<Vec<Gate>> {
            Err(PluginError::Other("scan down".into()))
        }
        fn mint_gate(&self, _: &str, _: &str) -> Result<String> {
            unreachable!()
        }
        fn close_gate(&self, _: &str, _: &str) -> Result<()> {
            unreachable!()
        }
        fn merged_pr(&self, _: &str) -> Result<Option<String>> {
            unreachable!()
        }
    }
    assert!(dispatch("sync", "post", false, &Broken, &Ctx::default()).is_err());
    assert!(dispatch("claim", "post", false, &Broken, &ctx()).is_err());
}
