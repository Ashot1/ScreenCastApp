//! Fixed Todoist API v1 authority and the four selected read operations for 17A.

pub const AUTHORITY: &str = "https://api.todoist.com:443";

pub(crate) const REGISTRY: crate::integration::Registry = crate::integration::Registry {
    headers: &[
        (reqwest::header::ACCEPT, "application/json"),
        (reqwest::header::USER_AGENT, "Ambient/0.1.0"),
    ],
    extracted: &[crate::integration::ResponseHeader::RetryAfter],
    ambiguous_statuses: &[401, 429, 503],
    error_body: &[429, 503],
};

/// Builds the URL of one fixed Todoist read; returns `None` for unsafe input.
pub fn url(request: &ambient_core::TodoistRequest) -> Option<reqwest::Url> {
    use ambient_core::TodoistRequest;
    let mut url = reqwest::Url::parse(AUTHORITY).ok()?;
    {
        let mut path = url.path_segments_mut().ok()?;
        path.pop_if_empty().push("api").push("v1");
        match request {
            TodoistRequest::User => {
                path.push("user");
            }
            TodoistRequest::Task { id } => {
                if !valid_id(id) {
                    return None;
                }
                path.push("tasks").push(id);
            }
            TodoistRequest::Project { id } => {
                if !valid_id(id) {
                    return None;
                }
                path.push("projects").push(id);
            }
            TodoistRequest::Comments { task_id, cursor } => {
                if !valid_id(task_id) || cursor.as_deref().is_some_and(|c| !valid_cursor(c)) {
                    return None;
                }
                path.push("comments");
            }
        }
    }
    if let TodoistRequest::Comments { task_id, cursor } = request {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("task_id", task_id)
            .append_pair("limit", "50");
        if let Some(cursor) = cursor {
            query.append_pair("cursor", cursor);
        }
    }
    Some(url)
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.chars().any(char::is_control)
        && value != "."
        && value != ".."
}

/// Matches the documented cursor pattern `^[0-9a-zA-Z_-]+\.[0-9a-zA-Z_-]+$`.
fn valid_cursor(value: &str) -> bool {
    let token = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    value.len() <= 4096
        && value
            .split_once('.')
            .is_some_and(|(left, right)| token(left) && token(right))
}

#[cfg(test)]
mod tests {
    use super::{AUTHORITY, REGISTRY, url};
    use crate::integration::ResponseHeader;
    use ambient_core::TodoistRequest;
    use reqwest::header::{ACCEPT, USER_AGENT};

    fn task(id: &str) -> TodoistRequest {
        TodoistRequest::Task { id: id.into() }
    }

    fn comments(id: &str, cursor: Option<&str>) -> TodoistRequest {
        TodoistRequest::Comments {
            task_id: id.into(),
            cursor: cursor.map(Into::into),
        }
    }

    fn display(request: &TodoistRequest) -> Result<String, &'static str> {
        url(request)
            .map(|url| url.to_string())
            .ok_or("valid Todoist URL")
    }

    #[test]
    fn registry_is_fixed() {
        assert_eq!(AUTHORITY, "https://api.todoist.com:443");
        assert_eq!(
            REGISTRY.headers,
            &[(ACCEPT, "application/json"), (USER_AGENT, "Ambient/0.1.0")]
        );
        assert!(matches!(REGISTRY.extracted, [ResponseHeader::RetryAfter]));
        assert_eq!(REGISTRY.ambiguous_statuses, &[401, 429, 503]);
        assert_eq!(REGISTRY.error_body, &[429, 503]);
    }

    #[test]
    fn builds_the_four_fixed_get_urls() -> Result<(), &'static str> {
        assert_eq!(
            display(&TodoistRequest::User)?,
            "https://api.todoist.com/api/v1/user"
        );
        assert_eq!(
            display(&task("6X7rM8997g3RQmvh"))?,
            "https://api.todoist.com/api/v1/tasks/6X7rM8997g3RQmvh"
        );
        assert_eq!(
            display(&TodoistRequest::Project { id: "p1".into() })?,
            "https://api.todoist.com/api/v1/projects/p1"
        );
        assert_eq!(
            display(&comments("t1", None))?,
            "https://api.todoist.com/api/v1/comments?task_id=t1&limit=50"
        );
        assert_eq!(
            display(&comments("t1", Some("abc_-1.DEF_-2")))?,
            "https://api.todoist.com/api/v1/comments?task_id=t1&limit=50&cursor=abc_-1.DEF_-2"
        );
        Ok(())
    }

    #[test]
    fn encodes_ids_as_one_opaque_segment() -> Result<(), &'static str> {
        let value = display(&task("A/B +%é"))?;
        assert_eq!(
            value,
            "https://api.todoist.com/api/v1/tasks/A%2FB%20+%25%C3%A9"
        );
        assert_eq!(
            display(&task("..%2Fsecret"))?,
            "https://api.todoist.com/api/v1/tasks/..%252Fsecret"
        );
        assert_eq!(
            display(&comments("A/B +%é", None))?,
            "https://api.todoist.com/api/v1/comments?task_id=A%2FB+%2B%25%C3%A9&limit=50"
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_ids() {
        let long = "a".repeat(257);
        for id in ["", ".", "..", "bad\nid", long.as_str()] {
            assert!(url(&task(id)).is_none(), "{id:?}");
            assert!(url(&TodoistRequest::Project { id: id.into() }).is_none());
            assert!(url(&comments(id, None)).is_none());
        }
    }

    #[test]
    fn accepts_maximum_id_size() {
        assert!(url(&task(&"é".repeat(128))).is_some());
    }

    #[test]
    fn rejects_invalid_cursors() {
        let long = format!("a.{}", "b".repeat(4095));
        for cursor in [
            "nodot",
            ".b",
            "a.",
            "a+b.c",
            "a.b.c",
            "a b.c",
            long.as_str(),
            "",
        ] {
            assert!(url(&comments("t1", Some(cursor))).is_none(), "{cursor:?}");
        }
        let max = format!("a.{}", "b".repeat(4094));
        assert!(url(&comments("t1", Some(&max))).is_some());
    }
}
