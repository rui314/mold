use std::path::Path;

#[test]
fn test_parse_check_line() {
    // =========================
    // ===== Success Cases =====
    // =========================

    // the basic case
    let crate::ParsedCheckLine {
        file_string,
        is_escaped,
        file_path,
        expected_hash,
    } = crate::parse_check_line(
        "0909090909090909090909090909090909090909090909090909090909090909  foo",
    )
    .unwrap();
    assert_eq!(expected_hash, blake3::Hash::from([0x09; 32]));
    assert!(!is_escaped);
    assert_eq!(file_string, "foo");
    assert_eq!(file_path, Path::new("foo"));

    // regular whitespace
    let crate::ParsedCheckLine {
        file_string,
        is_escaped,
        file_path,
        expected_hash,
    } = crate::parse_check_line(
        "fafafafafafafafafafafafafafafafafafafafafafafafafafafafafafafafa   \t\r\n\n\r \t\r\n\n\r",
    )
    .unwrap();
    assert_eq!(expected_hash, blake3::Hash::from([0xfa; 32]));
    assert!(!is_escaped);
    assert_eq!(file_string, " \t\r\n\n\r \t");
    assert_eq!(file_path, Path::new(" \t\r\n\n\r \t"));

    // path is one space
    let crate::ParsedCheckLine {
        file_string,
        is_escaped,
        file_path,
        expected_hash,
    } = crate::parse_check_line(
        "4242424242424242424242424242424242424242424242424242424242424242   ",
    )
    .unwrap();
    assert_eq!(expected_hash, blake3::Hash::from([0x42; 32]));
    assert!(!is_escaped);
    assert_eq!(file_string, " ");
    assert_eq!(file_path, Path::new(" "));

    // *Unescaped* backslashes. Note that this line does *not* start with a
    // backslash, so something like "\" + "n" is interpreted as *two*
    // characters. We forbid all backslashes on Windows, so this test is
    // Unix-only.
    if cfg!(not(windows)) {
        let crate::ParsedCheckLine {
            file_string,
            is_escaped,
            file_path,
            expected_hash,
        } = crate::parse_check_line(
            "4343434343434343434343434343434343434343434343434343434343434343  fo\\a\\no",
        )
        .unwrap();
        assert_eq!(expected_hash, blake3::Hash::from([0x43; 32]));
        assert!(!is_escaped);
        assert_eq!(file_string, "fo\\a\\no");
        assert_eq!(file_path, Path::new("fo\\a\\no"));
    }

    // escaped newlines
    let crate::ParsedCheckLine {
        file_string,
        is_escaped,
        file_path,
        expected_hash,
    } = crate::parse_check_line(
        "\\4444444444444444444444444444444444444444444444444444444444444444  fo\\r\\n\\n\\ro",
    )
    .unwrap();
    assert_eq!(expected_hash, blake3::Hash::from([0x44; 32]));
    assert!(is_escaped);
    assert_eq!(file_string, "fo\\r\\n\\n\\ro");
    assert_eq!(file_path, Path::new("fo\r\n\n\ro"));

    // Escaped newline and backslash. Again because backslash is not allowed on
    // Windows, this test is Unix-only.
    if cfg!(not(windows)) {
        let crate::ParsedCheckLine {
            file_string,
            is_escaped,
            file_path,
            expected_hash,
        } = crate::parse_check_line(
            "\\4545454545454545454545454545454545454545454545454545454545454545  fo\\n\\\\o",
        )
        .unwrap();
        assert_eq!(expected_hash, blake3::Hash::from([0x45; 32]));
        assert!(is_escaped);
        assert_eq!(file_string, "fo\\n\\\\o");
        assert_eq!(file_path, Path::new("fo\n\\o"));
    }

    // non-ASCII path
    let crate::ParsedCheckLine {
        file_string,
        is_escaped,
        file_path,
        expected_hash,
    } = crate::parse_check_line(
        "4646464646464646464646464646464646464646464646464646464646464646  否认",
    )
    .unwrap();
    assert_eq!(expected_hash, blake3::Hash::from([0x46; 32]));
    assert!(!is_escaped);
    assert_eq!(file_string, "否认");
    assert_eq!(file_path, Path::new("否认"));

    // untagged separator "  " in the file name
    let crate::ParsedCheckLine {
        file_string,
        is_escaped,
        file_path,
        expected_hash,
    } = crate::parse_check_line(
        "4747474747474747474747474747474747474747474747474747474747474747  foo  bar",
    )
    .unwrap();
    assert_eq!(expected_hash, blake3::Hash::from([0x47; 32]));
    assert!(!is_escaped);
    assert_eq!(file_string, "foo  bar");
    assert_eq!(file_path, Path::new("foo  bar"));

    // tagged separator ") = " in the file name
    let crate::ParsedCheckLine {
        file_string,
        is_escaped,
        file_path,
        expected_hash,
    } = crate::parse_check_line(
        "BLAKE3 (foo) = bar) = 4848484848484848484848484848484848484848484848484848484848484848",
    )
    .unwrap();
    assert_eq!(expected_hash, blake3::Hash::from([0x48; 32]));
    assert!(!is_escaped);
    assert_eq!(file_string, "foo) = bar");
    assert_eq!(file_path, Path::new("foo) = bar"));

    // =========================
    // ===== Failure Cases =====
    // =========================

    // too short
    crate::parse_check_line("").unwrap_err();
    crate::parse_check_line("0").unwrap_err();
    crate::parse_check_line("00").unwrap_err();
    crate::parse_check_line("0000000000000000000000000000000000000000000000000000000000000000")
        .unwrap_err();
    crate::parse_check_line("0000000000000000000000000000000000000000000000000000000000000000  ")
        .unwrap_err();

    // not enough spaces
    crate::parse_check_line("0000000000000000000000000000000000000000000000000000000000000000 foo")
        .unwrap_err();

    // capital letter hex
    crate::parse_check_line(
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA  foo",
    )
    .unwrap_err();

    // non-hex hex
    crate::parse_check_line(
        "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx  foo",
    )
    .unwrap_err();

    // non-ASCII hex
    crate::parse_check_line("你好, 我叫杰克. 认识你很高兴. 要不要吃个香蕉?  foo").unwrap_err();

    // invalid escape sequence
    crate::parse_check_line(
        "\\0000000000000000000000000000000000000000000000000000000000000000  fo\\o",
    )
    .unwrap_err();

    // truncated escape sequence
    crate::parse_check_line(
        "\\0000000000000000000000000000000000000000000000000000000000000000  foo\\",
    )
    .unwrap_err();

    // null char
    crate::parse_check_line(
        "0000000000000000000000000000000000000000000000000000000000000000  fo\0o",
    )
    .unwrap_err();

    // Unicode replacement char
    crate::parse_check_line(
        "0000000000000000000000000000000000000000000000000000000000000000  fo�o",
    )
    .unwrap_err();

    // On Windows only, backslashes are not allowed, escaped or otherwise.
    if cfg!(windows) {
        crate::parse_check_line(
            "0000000000000000000000000000000000000000000000000000000000000000  fo\\o",
        )
        .unwrap_err();
        crate::parse_check_line(
            "\\0000000000000000000000000000000000000000000000000000000000000000  fo\\\\o",
        )
        .unwrap_err();
    }
}

#[test]
fn test_filepath_to_string() {
    let output = crate::filepath_to_string(Path::new("foo"));
    assert_eq!(output.filepath_string, "foo");
    assert!(!output.is_escaped);

    let output = crate::filepath_to_string(Path::new("f\\ \t\r\noo"));
    if cfg!(windows) {
        // We normalize backslashes to forward slashes on Windows.
        assert_eq!(output.filepath_string, "f/ \t\\r\\noo");
    } else {
        assert_eq!(output.filepath_string, "f\\\\ \t\\r\\noo");
    }
    assert!(output.is_escaped);
}
