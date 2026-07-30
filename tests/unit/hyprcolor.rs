use super::*;

#[test]
fn extracts_hex_from_hyprcolor_json() {
    let json = r##"{ "wallpaper": "/x/y.png", "background": "#0d0f14", "accent": "#9a8cff" }"##;

    assert_eq!(extract_hex(json, "background").as_deref(), Some("#0d0f14"));
    assert_eq!(extract_hex(json, "accent").as_deref(), Some("#9a8cff"));
    assert_eq!(extract_hex(json, "missing"), None);
}

#[test]
fn parses_valid_hex_colors() {
    assert_eq!(parse_hex("#0d0f14"), Some((0x0d, 0x0f, 0x14)));
    assert_eq!(parse_hex("#ffffff"), Some((255, 255, 255)));
}

#[test]
fn rejects_malformed_hex_colors() {
    assert_eq!(parse_hex("0d0f14"), None);
    assert_eq!(parse_hex("#fff"), None);
    assert_eq!(parse_hex("#zzzzzz"), None);
    assert_eq!(parse_hex("#aébcd"), None);
}
