//! The time window every ranged statement narrows by, written once: a listing, a search and a purge
//! all bound the same two placeholders over their own timestamp column.

/// SQL narrowing `column` to `[first, first + 1]`: the two placeholders carry `since` and `until`,
/// and a NULL bound leaves that side unbounded. The upper end is inclusive.
pub(crate) fn predicate(column: &str, first: usize) -> String {
    format!(
        "(${first}::timestamptz IS NULL OR {column} >= ${first}) \
         AND (${next}::timestamptz IS NULL OR {column} <= ${next})",
        next = first + 1,
    )
}

/// The same window as a clause an existing `WHERE` can append.
pub(crate) fn time_window(column: &str, first: usize) -> String {
    format!("AND {}", predicate(column, first))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_bounds_its_column_with_the_two_placeholders_it_was_given() {
        assert_eq!(
            time_window("created_at", 2),
            "AND ($2::timestamptz IS NULL OR created_at >= $2) \
             AND ($3::timestamptz IS NULL OR created_at <= $3)"
        );
        assert_eq!(
            time_window("COALESCE(sent_at, created_at)", 5),
            "AND ($5::timestamptz IS NULL OR COALESCE(sent_at, created_at) >= $5) \
             AND ($6::timestamptz IS NULL OR COALESCE(sent_at, created_at) <= $6)"
        );
    }

    /// The clause form is the bare predicate with its `AND` in front, so a statement that has to
    /// combine the window with an `OR` reads the same two bounds.
    #[test]
    fn the_clause_form_is_the_bare_predicate() {
        assert_eq!(
            time_window("created_at", 3),
            format!("AND {}", predicate("created_at", 3))
        );
    }
}
