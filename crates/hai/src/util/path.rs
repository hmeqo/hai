pub fn sanitize_path(s: &str) -> String {
    sanitize_filename::sanitize(s)
}
