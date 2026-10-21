//! Fixed GitHub.com REST authority, headers and approved read paths of the GitHub adapter.

const AUTHORITY: &str = "https://api.github.com:443";

// `x-github-api-version` is a custom HeaderName, which holds interior-mutable bytes; a
// borrowed temporary array of it cannot live in a const, so the headers are a named static.
static HEADERS: [(reqwest::header::HeaderName, &str); 3] = [
    (reqwest::header::ACCEPT, "application/vnd.github+json"),
    (reqwest::header::USER_AGENT, "Ambient/0.1.0"),
    (
        reqwest::header::HeaderName::from_static("x-github-api-version"),
        "2026-03-10",
    ),
];

pub(crate) static REGISTRY: crate::integration::Registry = crate::integration::Registry {
    headers: &HEADERS,
    extracted: &[
        crate::integration::ResponseHeader::Link,
        crate::integration::ResponseHeader::RetryAfter,
        crate::integration::ResponseHeader::RateRemaining,
        crate::integration::ResponseHeader::RateReset,
    ],
    ambiguous_statuses: &[401, 403, 429],
    error_body: &[],
};

/// Builds a URL only for an approved GitHub read category.
///
/// `category` is 0 for PR detail, 1 for reviews, 2 for inline comments, and 3
/// for general issue comments. Every category requires `page` in 1..=5; only
/// the list categories (1..=3) carry it in the query.
pub(crate) fn url(request: &ambient_core::GitHubRequest) -> Option<reqwest::Url> {
    let ambient_core::GitHubRequest {
        owner,
        repository,
        number,
        category,
        page,
    } = request;
    if *number <= 0
        || !valid_segment(owner)
        || !valid_segment(repository)
        || !(1..=5).contains(page)
    {
        return None;
    }

    let (collection, suffix) = match category {
        0 => ("pulls", None),
        1 => ("pulls", Some("reviews")),
        2 => ("pulls", Some("comments")),
        3 => ("issues", Some("comments")),
        _ => return None,
    };

    let mut url = reqwest::Url::parse(AUTHORITY).ok()?;
    {
        // Segments are validated above, so pushing cannot introduce separators.
        let mut path = url.path_segments_mut().ok()?;
        path.pop_if_empty()
            .push("repos")
            .push(owner)
            .push(repository)
            .push(collection)
            .push(&number.to_string());
        if let Some(suffix) = suffix {
            path.push(suffix);
        }
    }
    if suffix.is_some() {
        url.query_pairs_mut()
            .append_pair("per_page", "100")
            .append_pair("page", &page.to_string());
    }
    Some(url)
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::{REGISTRY, url};
    use crate::integration::ResponseHeader;
    use ambient_core::GitHubRequest;
    use reqwest::header::{ACCEPT, HeaderName, USER_AGENT};

    fn request(
        owner: &str,
        repository: &str,
        number: i64,
        category: u32,
        page: u32,
    ) -> GitHubRequest {
        GitHubRequest {
            owner: owner.to_owned(),
            repository: repository.to_owned(),
            number,
            category,
            page,
        }
    }

    fn display(
        owner: &str,
        repository: &str,
        number: i64,
        category: u32,
        page: u32,
    ) -> Option<String> {
        url(&request(owner, repository, number, category, page)).map(|url| url.to_string())
    }

    #[test]
    fn builds_only_the_four_approved_urls() {
        assert_eq!(
            REGISTRY.headers,
            &[
                (ACCEPT, "application/vnd.github+json"),
                (USER_AGENT, "Ambient/0.1.0"),
                (
                    HeaderName::from_static("x-github-api-version"),
                    "2026-03-10"
                ),
            ]
        );
        assert!(matches!(
            REGISTRY.extracted,
            [
                ResponseHeader::Link,
                ResponseHeader::RetryAfter,
                ResponseHeader::RateRemaining,
                ResponseHeader::RateReset
            ]
        ));
        assert_eq!(REGISTRY.ambiguous_statuses, &[401, 403, 429]);
        assert!(REGISTRY.error_body.is_empty());
        assert_eq!(
            display("octo-org", "screen_cast", 42, 0, 1).as_deref(),
            Some("https://api.github.com/repos/octo-org/screen_cast/pulls/42")
        );
        assert_eq!(
            display("Example", "Project", 3, 1, 2).as_deref(),
            Some(
                "https://api.github.com/repos/Example/Project/pulls/3/reviews?per_page=100&page=2"
            )
        );
        assert_eq!(
            display("octo-org", "screen_cast", 42, 2, 5).as_deref(),
            Some(
                "https://api.github.com/repos/octo-org/screen_cast/pulls/42/comments?per_page=100&page=5"
            )
        );
        assert_eq!(
            display("octo-org", "screen_cast", 42, 3, 1).as_deref(),
            Some(
                "https://api.github.com/repos/octo-org/screen_cast/issues/42/comments?per_page=100&page=1"
            )
        );
        assert_eq!(display("octo-org", "screen_cast", 42, 4, 1), None);
    }

    #[test]
    fn rejects_invalid_numbers_segments_categories_and_pages() {
        assert_eq!(display("octo-org", "screen_cast", 0, 0, 1), None);
        assert_eq!(display("octo-org", "screen_cast", -1, 0, 1), None);
        assert_eq!(display("", "screen_cast", 42, 0, 1), None);
        assert_eq!(display(".", "screen_cast", 42, 0, 1), None);
        assert_eq!(display("..", "screen_cast", 42, 0, 1), None);
        assert_eq!(display("org/name", "screen_cast", 42, 0, 1), None);
        assert_eq!(display("org", "repo name", 42, 0, 1), None);
        assert_eq!(display("org", "repo?x=1", 42, 0, 1), None);
        assert_eq!(display("org", "repo", 42, 99, 1), None);
        for category in 0..=3 {
            assert_eq!(display("org", "repo", 42, category, 0), None);
            assert_eq!(display("org", "repo", 42, category, 6), None);
        }
    }

    #[test]
    fn accepts_segment_boundary_and_allowed_ascii() {
        let max_segment = "a".repeat(100);
        assert!(display(&max_segment, "R_e.po-1", i64::MAX, 0, 1).is_some());
        let too_long = format!("{}a", max_segment);
        assert_eq!(display(&too_long, "repo", 42, 0, 1), None);
        assert_eq!(display("orgé", "repo", 42, 0, 1), None);
    }
}
