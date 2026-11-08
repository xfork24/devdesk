use devdesk_adapters::github::GitHubPullRequests;
use devdesk_core::ports::PullRequests;
use devdesk_domain::PullRequestRequest;

#[test]
#[ignore = "requires an explicit existing dedicated acceptance PR request and real gh credentials"]
fn lost_pr_receipt_is_recovered_by_the_original_marker_without_creating_a_second_pr() {
    let path =
        std::env::var("DEVDESK_LIVE_PR_REQUEST").expect("set an explicit receipt request path");
    let mut request: PullRequestRequest =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        request.repository,
        std::env::var("DEVDESK_LIVE_GITHUB_REPO").expect("set the dedicated test repository")
    );
    assert_ne!(request.repository, "xfork24/devdesk");
    assert!(
        request.number.is_some(),
        "read a known existing PR before testing lost receipt recovery"
    );
    let adapter = GitHubPullRequests;
    let known = adapter.read(&request).unwrap();
    request.number = None;
    let recovered = adapter.ensure(&request).unwrap();
    let again = adapter.ensure(&request).unwrap();
    assert_eq!(known.number, recovered.number);
    assert_eq!(known.number, again.number);
    assert_eq!(known.head, recovered.head);
    assert_eq!(known.marker, recovered.marker);
    assert_eq!(known.merged, recovered.merged);
}
