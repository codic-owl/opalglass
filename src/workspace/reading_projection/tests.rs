use super::{Projection, Row};

#[test]
fn enormous_documents_remain_one_index_run() {
    let projection = Projection::identity(1_000_000_000);
    assert_eq!(projection.runs.len(), 1);
    assert_eq!(projection.len(), 1_000_000_000);
    for index in [0, 9, 999_999_999] {
        assert_eq!(
            projection.row(index),
            Some(Row::Document { instance: 0, index })
        );
        assert_eq!(projection.host_row(index), index);
        assert_eq!(projection.list_index(index), index);
    }
    assert_eq!(projection.row(1_000_000_000), None);
}

fn composed() -> Projection {
    let mut projection = Projection::default();
    projection.rows(0, 0..3, 0);
    projection.push(Row::Header { instance: 7 }, 3);
    projection.rows(7, 50..500_050, 3);
    projection.push(Row::Footer { instance: 7 }, 3);
    projection.rows(0, 4..10, 4);
    projection
}

#[test]
fn nested_content_maps_to_its_host_without_a_per_row_reverse_map() {
    let projection = composed();
    assert_eq!(projection.runs.len(), 5);
    assert_eq!(projection.host_row(300_000), 3);
    assert_eq!(projection.list_index(3), 3);
    assert_eq!(projection.list_index(4), 500_005);
    assert_eq!(
        projection.row(4),
        Some(Row::Document {
            instance: 7,
            index: 50
        })
    );
    assert_eq!(
        projection.row(500_005),
        Some(Row::Document {
            instance: 0,
            index: 4
        })
    );
    assert_eq!(projection.find_document_row(7, 300_000), Some(299_954));
}

#[test]
fn expanding_above_viewport_preserves_inner_and_outer_anchors() {
    let before = composed();
    let anchor = before.anchor(300_000, 6.5).unwrap();
    let mut after = Projection::default();
    after.rows(0, 0..1, 0);
    after.push(Row::Header { instance: 9 }, 1);
    after.rows(9, 0..100_000, 1);
    after.push(Row::Footer { instance: 9 }, 1);
    after.rows(0, 2..3, 2);
    after.push(Row::Header { instance: 7 }, 3);
    after.rows(7, 50..500_050, 3);
    after.push(Row::Footer { instance: 7 }, 3);
    after.rows(0, 4..10, 4);
    assert_eq!(after.restore(&anchor), 400_001);
    assert_eq!(after.row(after.restore(&anchor)), Some(anchor.row));
    assert_eq!(anchor.offset, 6.5);
}

#[test]
fn evicted_document_falls_back_to_its_embedding_location() {
    let before = composed();
    let anchor = before.anchor(30, 2.).unwrap();
    let mut after = Projection::default();
    after.rows(0, 0..3, 0);
    after.push(Row::Placeholder { instance: 7 }, 3);
    after.rows(0, 4..10, 4);
    assert_eq!(after.restore(&anchor), 3);
    assert_eq!(after.host_row(3), 3);
}

#[test]
fn alias_spanning_many_host_rows_maps_to_its_replacement() {
    let mut projection = Projection::default();
    projection.rows(0, 0..2, 0);
    let first = projection.len();
    projection.push(Row::Header { instance: 7 }, 2);
    projection.rows(7, 0..500_000, 2);
    projection.push(Row::Footer { instance: 7 }, 2);
    projection.cover_host_rows(first, 2..100_002);
    projection.rows(0, 100_002..100_010, 100_002);
    for host in [2, 3, 50_000, 100_001] {
        assert_eq!(projection.list_index(host), first);
    }
    assert_eq!(projection.list_index(100_002), 500_004);
    assert_eq!(projection.host_row(500_002), 2);
    assert_eq!(projection.runs.len(), 5);
}
