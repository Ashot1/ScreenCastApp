//! Fixed Jira Cloud gateway authority and selected read operations for 12A.

pub const AUTHORITY: &str = "https://api.atlassian.com:443";

pub(crate) const REGISTRY: crate::integration::Registry = crate::integration::Registry {
    headers: &[
        (reqwest::header::ACCEPT, "application/json"),
        (reqwest::header::USER_AGENT, "Ambient/0.1.0"),
    ],
    extracted: &[crate::integration::ResponseHeader::RetryAfter],
    ambiguous_statuses: &[401, 429, 503],
};

/// Builds one of the selected Jira Cloud read URLs.
///
/// `category` is 0 for issue details, 1 for comments, and 2 for changelog.
pub fn url(
    cloud_id: &str,
    issue: &str,
    category: u32,
    start_at: i64,
    max_results: i32,
) -> Option<reqwest::Url> {
    if !valid_cloud_id(cloud_id) || !valid_issue(issue) {
        return None;
    }

    let endpoint = match category {
        0 => "issue",
        1 if start_at >= 0 && max_results == 50 => "comment",
        2 if (0..=i32::MAX as i64).contains(&start_at) && (1..=50).contains(&max_results) => {
            "changelog"
        }
        _ => return None,
    };

    let mut url = reqwest::Url::parse(AUTHORITY).ok()?;
    {
        let mut path = url.path_segments_mut().ok()?;
        path.pop_if_empty()
            .push("ex")
            .push("jira")
            .push(cloud_id)
            .push("rest")
            .push("api")
            .push("3")
            .push("issue")
            .push(issue);
        if category != 0 {
            path.push(endpoint);
        }
    }

    let mut query = url.query_pairs_mut();
    match category {
        0 => {
            query
                .append_pair(
                    "fields",
                    "summary,description,status,issuetype,project,created,updated,parent,subtasks,issuelinks",
                )
                .append_pair("updateHistory", "false")
                .append_pair("failFast", "false");
        }
        1 => {
            query
                .append_pair("startAt", &start_at.to_string())
                .append_pair("maxResults", "50")
                .append_pair("orderBy", "-created");
        }
        2 => {
            query
                .append_pair("startAt", &start_at.to_string())
                .append_pair("maxResults", &max_results.to_string());
        }
        _ => return None,
    }
    drop(query);
    Some(url)
}

fn valid_cloud_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_issue(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.chars().any(char::is_control)
        && value != "."
        && value != ".."
}

#[cfg(test)]
mod tests {
    use super::{AUTHORITY, REGISTRY, url};
    use crate::integration::ResponseHeader;
    use reqwest::header::{ACCEPT, USER_AGENT};

    fn display(
        category: u32,
        issue: &str,
        start_at: i64,
        max_results: i32,
    ) -> Result<String, &'static str> {
        url("Cloud-ID-123", issue, category, start_at, max_results)
            .map(|url| url.to_string())
            .ok_or("valid Jira URL")
    }

    #[test]
    fn builds_the_three_fixed_get_urls() -> Result<(), &'static str> {
        assert_eq!(AUTHORITY, "https://api.atlassian.com:443");
        assert_eq!(
            REGISTRY.headers,
            &[(ACCEPT, "application/json"), (USER_AGENT, "Ambient/0.1.0")]
        );
        assert!(matches!(REGISTRY.extracted, [ResponseHeader::RetryAfter]));
        assert_eq!(REGISTRY.ambiguous_statuses, &[401, 429, 503]);
        assert_eq!(
            display(0, "PROJ-7", 0, 50)?,
            "https://api.atlassian.com/ex/jira/Cloud-ID-123/rest/api/3/issue/PROJ-7?fields=summary%2Cdescription%2Cstatus%2Cissuetype%2Cproject%2Ccreated%2Cupdated%2Cparent%2Csubtasks%2Cissuelinks&updateHistory=false&failFast=false"
        );
        assert_eq!(
            display(1, "10001", 50, 50)?,
            "https://api.atlassian.com/ex/jira/Cloud-ID-123/rest/api/3/issue/10001/comment?startAt=50&maxResults=50&orderBy=-created"
        );
        assert_eq!(
            display(2, "10001", 7, 1)?,
            "https://api.atlassian.com/ex/jira/Cloud-ID-123/rest/api/3/issue/10001/changelog?startAt=7&maxResults=1"
        );
        Ok(())
    }

    #[test]
    fn encodes_issue_selection_as_one_opaque_segment() -> Result<(), &'static str> {
        let value = display(0, "A/B +%é", 0, 50)?;
        assert!(value.contains("/issue/A%2FB%20+%25%C3%A9?"), "{value}");
        assert_eq!(
            display(0, "..%2Fsecret", 0, 50)?
                .split("/rest/api/3/")
                .nth(1)
                .and_then(|part| part.split('?').next()),
            Some("issue/..%252Fsecret")
        );
        assert_eq!(
            display(0, "%2e%2e", 0, 50)?
                .split("/rest/api/3/")
                .nth(1)
                .and_then(|part| part.split('?').next()),
            Some("issue/%252e%252e")
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_identifiers_categories_and_paging() {
        assert!(url("", "PROJ-7", 0, 0, 50).is_none());
        assert!(url(&"a".repeat(129), "PROJ-7", 0, 0, 50).is_none());
        assert!(url("Cloud_ID", "PROJ-7", 0, 0, 50).is_none());
        assert!(url("Cloud-ID", "", 0, 0, 50).is_none());
        assert!(url("Cloud-ID", &"a".repeat(257), 0, 0, 50).is_none());
        assert!(url("Cloud-ID", "bad\nkey", 0, 0, 50).is_none());
        assert!(url("Cloud-ID", "PROJ-7", 3, 0, 50).is_none());
        assert!(url("Cloud-ID", "PROJ-7", 1, -1, 50).is_none());
        assert!(url("Cloud-ID", "PROJ-7", 1, 0, 49).is_none());
        assert!(url("Cloud-ID", "PROJ-7", 2, -1, 1).is_none());
        assert!(url("Cloud-ID", "PROJ-7", 2, i32::MAX as i64 + 1, 1).is_none());
        assert!(url("Cloud-ID", "PROJ-7", 2, 0, 0).is_none());
        assert!(url("Cloud-ID", "PROJ-7", 2, 0, 51).is_none());
    }

    #[test]
    fn accepts_identifier_and_paging_limits() -> Result<(), &'static str> {
        let cloud_id = "x".repeat(128);
        let issue = "é".repeat(128);
        url(&cloud_id, &issue, 0, 0, 50).ok_or("maximum identifier sizes")?;
        url("Cloud-ID", "PROJ-7", 1, i64::MAX, 50).ok_or("maximum comment offset")?;
        url("Cloud-ID", "PROJ-7", 2, i32::MAX as i64, 50).ok_or("maximum changelog offset")?;
        Ok(())
    }

    #[test]
    fn rejects_dot_segments_that_the_url_parser_would_normalize() {
        assert!(url("Cloud-ID", ".", 0, 0, 50).is_none());
        assert!(url("Cloud-ID", "..", 0, 0, 50).is_none());
    }
}
