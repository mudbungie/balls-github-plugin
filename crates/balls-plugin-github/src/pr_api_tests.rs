use super::*;

const UA: &str = "balls-plugin-github-test";

#[test]
fn find_pr_none_then_one() {
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
        .with_status(200)
        .with_body("[]")
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    assert!(find_pr(&c, "o", "n", "work/bl-1").unwrap().is_none());

    let mut s2 = mockito::Server::new();
    s2.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
        .with_status(200)
        .with_body(r#"[{"number":4,"html_url":"https://gh/pr/4","merged":true}]"#)
        .create();
    let c2 = GithubClient::new(&s2.url(), "t", UA);
    let pr = find_pr(&c2, "o", "n", "work/bl-1").unwrap().unwrap();
    assert_eq!(pr.number, 4);
    assert_eq!(pr.html_url, "https://gh/pr/4");
    assert!(pr.merged);
}

#[test]
fn create_pr_ok_and_error() {
    let mut s = mockito::Server::new();
    s.mock("POST", "/repos/o/n/pulls")
        .with_status(201)
        .with_body(r#"{"number":9,"html_url":"https://gh/pr/9"}"#)
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    let pr = create_pr(&c, "o", "n", "T [bl-1]", "o:work/bl-1", "main", "b").unwrap();
    assert_eq!(pr.number, 9);
    assert!(!pr.merged);

    let mut s2 = mockito::Server::new();
    s2.mock("POST", "/repos/o/n/pulls").with_status(422).with_body("no commits").create();
    let c2 = GithubClient::new(&s2.url(), "t", UA);
    assert!(create_pr(&c2, "o", "n", "t", "h", "main", "b").is_err());
}

#[test]
fn close_pr_patches_state() {
    let mut s = mockito::Server::new();
    let m = s
        .mock("PATCH", "/repos/o/n/pulls/12")
        .match_body(mockito::Matcher::PartialJsonString(r#"{"state":"closed"}"#.into()))
        .with_status(200)
        .with_body(r#"{"number":12,"html_url":"u"}"#)
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    close_pr(&c, "o", "n", 12).unwrap();
    m.assert();
}

#[test]
fn close_pr_propagates_api_error() {
    let mut s = mockito::Server::new();
    s.mock("PATCH", "/repos/o/n/pulls/12").with_status(404).with_body("gone").create();
    let c = GithubClient::new(&s.url(), "t", UA);
    assert!(close_pr(&c, "o", "n", 12).is_err());
}

#[test]
fn push_url_public_and_enterprise() {
    assert_eq!(push_url("https://api.github.com", "o/n", "TKN"), "https://x-access-token:TKN@github.com/o/n.git");
    assert_eq!(push_url("https://ghe.x/api/v3", "o/n", "TKN"), "https://x-access-token:TKN@ghe.x/o/n.git");
    // trailing scheme variations
    assert_eq!(push_url("http://localhost:8080", "a/b", "T"), "https://x-access-token:T@localhost:8080/a/b.git");
}
