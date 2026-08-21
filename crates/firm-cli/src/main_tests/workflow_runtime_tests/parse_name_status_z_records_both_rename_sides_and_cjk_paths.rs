use super::*;

#[test]
fn parse_name_status_z_records_both_rename_sides_and_cjk_paths() {
    // R100\0a.txt\0b.txt\0M\0keep.txt\0A\0<cjk>.txt (raw UTF-8, NUL-delimited).
    let bytes = b"R100\0a.txt\0b.txt\0M\0keep.txt\0A\0\xe6\x96\x87\xe4\xbb\xb6.txt\0";
    let paths = parse_name_status_z(bytes);
    assert!(
        paths.contains(&"a.txt".to_string()),
        "rename OLD side recorded"
    );
    assert!(
        paths.contains(&"b.txt".to_string()),
        "rename NEW side recorded"
    );
    assert!(paths.contains(&"keep.txt".to_string()));
    assert!(
        paths.contains(&"文件.txt".to_string()),
        "CJK path decoded raw (no c-quoting) from -z output"
    );
    assert_eq!(paths.len(), 4);
}
