use chat2db_storage::{
    MAX_RESULT_PAGE_BYTES, MAX_RESULT_PAGE_ROWS, MIN_RESULT_PAGE_BYTES, PageRequest, PurgeReport,
};

#[test]
fn paging_and_purge_contracts_are_nameable_outside_the_crate() {
    let request = PageRequest {
        offset: 7,
        max_rows: MAX_RESULT_PAGE_ROWS,
        max_bytes: MAX_RESULT_PAGE_BYTES,
    };
    let report = PurgeReport::default();

    assert_eq!(request.offset, 7);
    assert!(request.max_bytes >= MIN_RESULT_PAGE_BYTES);
    assert_eq!(report.results_removed, 0);
}
