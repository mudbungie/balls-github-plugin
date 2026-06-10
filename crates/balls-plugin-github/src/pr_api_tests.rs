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
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("head".into(), "o:work/bl-1".into()),
            mockito::Matcher::UrlEncoded("state".into(), "all".into()),
        ]))
        .with_status(200)
        .with_body(r#"[{"html_url":"https://gh/pr/4","merged_at":"2026-06-09T00:00:00Z"}]"#)
        .create();
    let c2 = GithubClient::new(&s2.url(), "t", UA);
    let pr = find_pr(&c2, "o", "n", "work/bl-1").unwrap().unwrap();
    assert_eq!(pr.html_url, "https://gh/pr/4");
    assert!(pr.merged());
}

#[test]
fn find_pr_propagates_api_errors() {
    let mut s = mockito::Server::new();
    s.mock("GET", mockito::Matcher::Regex(r"^/repos/o/n/pulls".into()))
        .with_status(500)
        .with_body("boom")
        .create();
    let c = GithubClient::new(&s.url(), "t", UA);
    assert!(find_pr(&c, "o", "n", "work/bl-1").is_err());
}
