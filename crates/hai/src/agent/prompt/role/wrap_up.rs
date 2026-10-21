pub(crate) fn extract_summary(response: &str) -> Option<String> {
    let start = response.find("<summary>")? + "<summary>".len();
    let rest = &response[start..];
    let end = rest.find("</summary>").unwrap_or(rest.len());
    let body = rest[..end].trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}
