/// 拼接多段 URL 路径，不计段头段尾斜杠。
pub fn join_url(segments: &[&str]) -> String {
    let mut url = String::new();
    for seg in segments {
        let seg = seg.trim_matches('/');
        if !seg.is_empty() {
            if !url.is_empty() {
                url.push('/');
            }
            url.push_str(seg);
        }
    }
    url
}

/// 保证 URL 末尾有斜杠（genai 需求）。
pub fn ensure_trailing_slash(mut url: String) -> String {
    if !url.is_empty() && !url.ends_with('/') {
        url.push('/');
    }
    url
}
