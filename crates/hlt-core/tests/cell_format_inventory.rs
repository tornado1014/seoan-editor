mod support;

use hlt_core::{CharacterFormat, inspect_cell_format_inventory};

const FIXTURE_SHA256: &str = "1932F167AA1D460FA91362CC46CE70CB16D6D2293E0ABE6FC94B2DA8899148C6";

#[test]
fn exposes_bold_headers_and_plain_body_by_stable_logical_cell_id() {
    let source = support::pct_table_fixture();
    let inventory = inspect_cell_format_inventory(source.path()).unwrap();

    assert_eq!(inventory.source_sha256, FIXTURE_SHA256);
    assert_eq!(inventory.cells.len(), 16);
    assert!(inventory.unresolved.is_empty());

    let first = inventory
        .cells
        .iter()
        .find(|cell| cell.cell_id == "cell:0:0:0")
        .unwrap();
    assert_eq!(first.table_id, "table:0");
    assert_eq!(first.text, "구성품");
    assert_eq!(first.spans.len(), 1);
    assert_eq!(first.spans[0].start, 0);
    assert_eq!(first.spans[0].end, 3);
    assert_eq!(first.spans[0].text, "구성품");
    assert_eq!(first.spans[0].raw_attr, 2);
    assert_eq!(first.spans[0].formats, vec![CharacterFormat::Bold]);

    let second = inventory
        .cells
        .iter()
        .find(|cell| cell.cell_id == "cell:0:0:1")
        .unwrap();
    assert_eq!(second.text, "t-값");
    assert_eq!(second.spans[0].start, 0);
    assert_eq!(second.spans[0].end, 3);
    assert_eq!(second.spans[0].raw_attr, 2);
    assert_eq!(second.spans[0].formats, vec![CharacterFormat::Bold]);

    let body = inventory
        .cells
        .iter()
        .find(|cell| cell.cell_id == "cell:0:1:0")
        .unwrap();
    assert_eq!(body.text, "재질");
    assert!(body.spans.is_empty());
}
