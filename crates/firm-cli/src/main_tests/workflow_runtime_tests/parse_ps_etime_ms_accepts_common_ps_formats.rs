use super::*;

#[test]
fn parse_ps_etime_ms_accepts_common_ps_formats() {
    assert_eq!(parse_ps_etime_ms("03"), Some(3_000));
    assert_eq!(parse_ps_etime_ms("02:03"), Some(123_000));
    assert_eq!(parse_ps_etime_ms("01:02:03"), Some(3_723_000));
    assert_eq!(parse_ps_etime_ms("2-01:02:03"), Some(176_523_000));
    assert_eq!(parse_ps_etime_ms(""), None);
    assert_eq!(parse_ps_etime_ms("bad"), None);
}
