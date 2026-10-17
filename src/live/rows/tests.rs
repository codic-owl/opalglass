use super::*;

#[test]
fn identity_projection_allocates_no_per_row_storage() {
    let rows = Rows::new(10_000_000, []);
    assert_eq!(rows.len(), 10_000_000);
    assert!(rows.gaps.is_empty());
    assert_eq!(rows.row(9_999_999), 9_999_999);
    assert_eq!(rows.row(usize::MAX), 9_999_999);
    assert_eq!(rows.list_index(usize::MAX), 9_999_999);
    assert!(rows.is_visible(9_999_999));
    assert!(!rows.is_visible(10_000_000));
}

#[test]
fn intervals_are_merged_clamped_and_sorted() {
    let rows = Rows::new(12, [8..100, 4..6, 2..5, 6..7, 30..40, 3..3]);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows.gaps.len(), 2);
    assert_eq!(rows.gaps[0].source, 2..7);
    assert_eq!(rows.gaps[1].source, 8..12);
    assert_eq!(
        (0..rows.len()).map(|i| rows.row(i)).collect::<Vec<_>>(),
        [0, 1, 7]
    );
    for source in 2..7 {
        assert_eq!(rows.list_index(source), 1);
        assert!(!rows.is_visible(source));
    }
    for source in 8..20 {
        assert_eq!(rows.list_index(source), 2);
    }
}

#[test]
fn leading_and_trailing_hidden_rows_resolve_to_visible_neighbors() {
    let rows = Rows::new(10, [0..3, 5..7, 9..10]);
    assert_eq!(
        (0..rows.len()).map(|i| rows.row(i)).collect::<Vec<_>>(),
        [3, 4, 7, 8]
    );
    assert_eq!(
        (0..10).map(|i| rows.list_index(i)).collect::<Vec<_>>(),
        [0, 0, 0, 0, 1, 1, 1, 2, 3, 3]
    );
    assert_eq!(rows.row(usize::MAX), 8);
}

#[test]
fn empty_and_fully_hidden_documents_have_no_rows() {
    for rows in [
        Rows::default(),
        Rows::new(0, std::iter::once(0..10)),
        Rows::new(10, std::iter::once(0..10)),
    ] {
        assert!(rows.is_empty());
        assert_eq!(rows.len(), 0);
        assert_eq!(rows.row(0), 0);
        assert_eq!(rows.row(usize::MAX), 0);
        assert_eq!(rows.list_index(0), 0);
        assert_eq!(rows.list_index(usize::MAX), 0);
        assert!(!rows.is_visible(0));
    }
}

#[test]
fn huge_documents_use_only_gap_metadata_and_do_not_overflow() {
    let count = usize::MAX;
    let rows = Rows::new(count, std::iter::once(1..count - 2));
    assert_eq!(rows.len(), 3);
    assert_eq!(rows.gaps.len(), 1);
    assert_eq!(rows.row(0), 0);
    assert_eq!(rows.row(1), count - 2);
    assert_eq!(rows.row(2), count - 1);
    assert_eq!(rows.row(usize::MAX), count - 1);
    assert_eq!(rows.list_index(count / 2), 0);
    assert_eq!(rows.list_index(usize::MAX), 2);
}

#[test]
fn compact_projection_matches_a_row_by_row_reference() {
    let mut random = 0x729b_38f4_265d_01ab_u64;
    let mut next = |bound: usize| {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        (random >> 32) as usize % bound
    };
    for _ in 0..500 {
        let count = next(128);
        let intervals: Vec<_> = (0..next(24)).map(|_| next(160)..next(160)).collect();
        let mut visible = vec![true; count];
        for range in &intervals {
            for row in visible.iter_mut().take(range.end).skip(range.start) {
                *row = false;
            }
        }
        let reference: Vec<_> = (0..count).filter(|&row| visible[row]).collect();
        let rows = Rows::new(count, intervals);
        assert_eq!(rows.len(), reference.len());
        for (index, &source) in reference.iter().enumerate() {
            assert_eq!(rows.row(index), source);
            assert_eq!(rows.list_index(source), index);
        }
        for source in 0..count + 10 {
            let expected = reference
                .partition_point(|&row| row <= source.min(count.saturating_sub(1)))
                .saturating_sub(1);
            assert_eq!(rows.list_index(source), expected);
            assert_eq!(rows.is_visible(source), visible.get(source) == Some(&true));
        }
    }
}
