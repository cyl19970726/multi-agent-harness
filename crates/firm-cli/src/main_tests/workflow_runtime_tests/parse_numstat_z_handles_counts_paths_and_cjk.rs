use super::*;

#[test]
fn parse_numstat_z_handles_counts_paths_and_cjk() {
    // `1\t1\trenamed.txt\0` `-\t-\timg.bin\0` `1\t0\t<cjk>.txt\0`.
    let bytes = b"1\t1\trenamed.txt\0-\t-\timg.bin\x001\t0\t\xe6\x96\x87\xe4\xbb\xb6.txt\0";
    let paths = parse_numstat_z(bytes);
    assert!(paths.contains(&"renamed.txt".to_string()));
    assert!(paths.contains(&"img.bin".to_string()));
    assert!(paths.contains(&"文件.txt".to_string()));
    assert_eq!(paths.len(), 3);
}
