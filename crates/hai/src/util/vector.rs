/// Cosine similarity between two vectors of equal length.
/// Returns 0.0–1.0 (higher = more similar). Returns None if lengths differ.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> Option<f64> {
    if a.len() != b.len() {
        return None;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum();
    let nb: f32 = b.iter().map(|x| x * x).sum();
    if na > 0.0 && nb > 0.0 {
        Some((dot / (na.sqrt() * nb.sqrt())) as f64)
    } else {
        Some(0.0)
    }
}

/// Deserialize a JSONB embedding array into `Vec<f32>`.
pub fn embedding_from_json(v: &serde_json::Value) -> Option<Vec<f32>> {
    v.as_array()?
        .iter()
        .map(|x| x.as_f64().map(|f| f as f32))
        .collect()
}
