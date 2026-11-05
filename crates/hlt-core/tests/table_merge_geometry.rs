//! 가로 병합 칸의 폭·칸 수. 열 폭을 환산할 때 생기는 반올림이 병합 칸 끝을 옆 행과 어긋나게 해도
//! 저장한 칸 수가 그대로 읽혀야 하고, 생성기는 애초에 어긋나지 않게 써야 한다.

use hlt_core::{
    GenerationDialect, HmlDocument, InspectDocument, InspectOptions, KipoExportOptions,
    WriteOptions, export_kipo_xml_path, inspect_kipo_xml_path, inspect_path,
    save_generated_hlt_as_new,
};
use std::path::{Path, PathBuf};

/// 5열 표 · 첫 행 = (1-2)(3-5). 이 열 폭은 합을 한 번에 환산하면 칸마다 환산한 합과 1 어긋난다.
const XML: &str = r#"<KIPO><PCTPatentCAFDOC><description><description-of-embodiments>
<p num="0001"><tables num="1"><table><tgroup cols="5">
<colspec colname="col1" colwidth="1822"/><colspec colname="col2" colwidth="1955"/><colspec colname="col3" colwidth="1688"/><colspec colname="col4" colwidth="1714"/><colspec colname="col5" colwidth="1929"/>
<tbody>
<row><entry namest="col1" nameend="col2">가</entry><entry namest="col3" nameend="col5">나</entry></row>
<row><entry colname="col1">다</entry><entry colname="col2">라</entry><entry colname="col3">마</entry><entry colname="col4">바</entry><entry colname="col5">사</entry></row>
</tbody></tgroup></table></tables></p>
</description-of-embodiments></description></PCTPatentCAFDOC></KIPO>"#;

fn source(dir: &tempfile::TempDir) -> (PathBuf, HmlDocument) {
    let path = dir.path().join("in.xml");
    std::fs::write(&path, XML).unwrap();
    let document = inspect_kipo_xml_path(&path).unwrap().document;
    (path, document)
}

fn bake(dir: &tempfile::TempDir, source: &Path, document: &HmlDocument) -> PathBuf {
    let output = dir.path().join("out.HLT");
    save_generated_hlt_as_new(
        source,
        document,
        GenerationDialect::PctPatent,
        &[],
        &output,
        WriteOptions::default(),
    )
    .unwrap();
    output
}

fn read(file: &Path) -> InspectDocument {
    inspect_path(file, InspectOptions::default()).unwrap()
}

fn row_spans(document: &InspectDocument, row: u32) -> Vec<u32> {
    document
        .cells
        .iter()
        .filter(|cell| cell.row == row)
        .map(|cell| cell.colspan)
        .collect()
}

fn row_end(document: &InspectDocument, row: u32) -> u32 {
    document
        .cells
        .iter()
        .filter(|cell| cell.row == row)
        .map(|cell| cell.start + cell.width)
        .max()
        .unwrap()
}

#[test]
fn generated_merged_cell_ends_on_the_column_edge() {
    let dir = tempfile::tempdir().unwrap();
    let (path, source) = source(&dir);
    let document = read(&bake(&dir, &path, &source));
    assert_eq!(row_end(&document, 0), row_end(&document, 1));
    assert_eq!(row_spans(&document, 0), [2, 3]);
    assert_eq!(document.tables[0].column_count, 5);
}

#[test]
fn export_keeps_the_merge_of_the_source_xml() {
    let dir = tempfile::tempdir().unwrap();
    let (path, source) = source(&dir);
    let file = bake(&dir, &path, &source);
    let xml = export_kipo_xml_path(&file, &KipoExportOptions::default())
        .unwrap()
        .xml;
    assert!(xml.contains(r#"namest="col3" nameend="col5""#), "{xml}");
    assert!(!xml.contains("col6"), "{xml}");
}

/// 생성기와 무관하게, 끝이 어긋난 파일도 저장한 칸 수로 읽는다(다른 도구가 쓴 파일 · 옛 판).
#[test]
fn reader_trusts_stored_spans_when_cell_edges_drift() {
    let dir = tempfile::tempdir().unwrap();
    let (path, mut document) = source(&dir);
    let merged = document
        .cells
        .iter_mut()
        .find(|cell| cell.colspan == 3)
        .unwrap();
    merged.width_raw = Some(4400);
    let file = bake(&dir, &path, &document);
    let read_back = read(&file);
    assert_ne!(row_end(&read_back, 0), row_end(&read_back, 1));
    assert_eq!(row_spans(&read_back, 0), [2, 3]);
    assert_eq!(read_back.tables[0].column_count, 5);
}
