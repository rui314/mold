use duct::cmd;
use std::ffi::OsString;
use std::fs;
use std::io::prelude::*;
use std::path::PathBuf;

pub fn b3sum_exe() -> PathBuf {
    env!("CARGO_BIN_EXE_b3sum").into()
}

#[test]
fn test_hash_one() {
    let expected = format!("{}  -", blake3::hash(b"foo").to_hex());
    let output = cmd!(b3sum_exe()).stdin_bytes("foo").read().unwrap();
    assert_eq!(&*expected, output);
}

#[test]
fn test_hash_one_raw() {
    let expected = blake3::hash(b"foo").as_bytes().to_owned();
    let output = cmd!(b3sum_exe(), "--raw")
        .stdin_bytes("foo")
        .stdout_capture()
        .run()
        .unwrap()
        .stdout;
    assert_eq!(expected, output.as_slice());
}

#[test]
fn test_hash_many() {
    let dir = tempfile::tempdir().unwrap();
    let file1 = dir.path().join("file1");
    fs::write(&file1, b"foo").unwrap();
    let file2 = dir.path().join("file2");
    fs::write(&file2, b"bar").unwrap();

    let output = cmd!(b3sum_exe(), &file1, &file2).read().unwrap();
    let foo_hash = blake3::hash(b"foo");
    let bar_hash = blake3::hash(b"bar");
    let expected = format!(
        "{}  {}\n{}  {}",
        foo_hash.to_hex(),
        // account for slash normalization on Windows
        file1.to_string_lossy().replace("\\", "/"),
        bar_hash.to_hex(),
        file2.to_string_lossy().replace("\\", "/"),
    );
    assert_eq!(expected, output);

    let output_no_names = cmd!(b3sum_exe(), "--no-names", &file1, &file2)
        .read()
        .unwrap();
    let expected_no_names = format!("{}\n{}", foo_hash.to_hex(), bar_hash.to_hex(),);
    assert_eq!(expected_no_names, output_no_names);
}

#[test]
fn test_missing_files() {
    let dir = tempfile::tempdir().unwrap();
    let file1 = dir.path().join("file1");
    fs::write(&file1, b"foo").unwrap();
    let file2 = dir.path().join("file2");
    fs::write(&file2, b"bar").unwrap();

    let output = cmd!(b3sum_exe(), "file1", "missing_file", "file2")
        .dir(dir.path())
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .unwrap();
    assert!(!output.status.success());

    let foo_hash = blake3::hash(b"foo");
    let bar_hash = blake3::hash(b"bar");
    let expected_stdout = format!(
        "{}  file1\n{}  file2\n",
        foo_hash.to_hex(),
        bar_hash.to_hex(),
    );
    assert_eq!(expected_stdout.as_bytes(), &output.stdout[..]);

    let bing_error = fs::File::open(dir.path().join("missing_file")).unwrap_err();
    let expected_stderr = format!("b3sum: missing_file: {}\n", bing_error.to_string());
    assert_eq!(expected_stderr.as_bytes(), &output.stderr[..]);
}

#[test]
fn test_hash_length_and_seek() {
    let mut expected = [0; 100];
    blake3::Hasher::new()
        .update(b"foo")
        .finalize_xof()
        .fill(&mut expected);
    let output = cmd!(b3sum_exe(), "--raw", "--length=100")
        .stdin_bytes("foo")
        .stdout_capture()
        .run()
        .unwrap()
        .stdout;
    assert_eq!(expected[..], output);

    let short_output = cmd!(b3sum_exe(), "--raw", "--length=99")
        .stdin_bytes("foo")
        .stdout_capture()
        .run()
        .unwrap()
        .stdout;
    assert_eq!(expected[..99], short_output);

    let seek1_output = cmd!(b3sum_exe(), "--raw", "--length=99", "--seek=1")
        .stdin_bytes("foo")
        .stdout_capture()
        .run()
        .unwrap()
        .stdout;
    assert_eq!(expected[1..], seek1_output);

    let seek99_output = cmd!(b3sum_exe(), "--raw", "--length=1", "--seek=99")
        .stdin_bytes("foo")
        .stdout_capture()
        .run()
        .unwrap()
        .stdout;
    assert_eq!(expected[99..], seek99_output);
}

#[test]
fn test_keyed() {
    let key = [42; blake3::KEY_LEN];
    let f = tempfile::NamedTempFile::new().unwrap();
    f.as_file().write_all(b"foo").unwrap();
    f.as_file().flush().unwrap();
    let expected = blake3::keyed_hash(&key, b"foo").to_hex();
    let output = cmd!(b3sum_exe(), "--keyed", "--no-names", f.path())
        .stdin_bytes(&key[..])
        .read()
        .unwrap();
    assert_eq!(&*expected, &*output);

    // Make sure that keys of the wrong length lead to errors.
    for bad_length in [0, 1, blake3::KEY_LEN - 1, blake3::KEY_LEN + 1] {
        dbg!(bad_length);
        let output = cmd!(b3sum_exe(), "--keyed", f.path())
            .stdin_bytes(vec![0; bad_length])
            .stdout_capture()
            .stderr_capture()
            .unchecked()
            .run()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        // Make sure the error message is relevant.
        let stderr = std::str::from_utf8(&output.stderr).unwrap();
        assert!(stderr.contains("key bytes"));
    }
}

#[test]
fn test_derive_key() {
    let context = "BLAKE3 2019-12-28 10:28:41 example context";
    let f = tempfile::NamedTempFile::new().unwrap();
    f.as_file().write_all(b"key material").unwrap();
    f.as_file().flush().unwrap();
    let expected = hex::encode(blake3::derive_key(context, b"key material"));
    let output = cmd!(b3sum_exe(), "--derive-key", context, "--no-names", f.path())
        .read()
        .unwrap();
    assert_eq!(&*expected, &*output);
}

#[test]
fn test_no_mmap() {
    let f = tempfile::NamedTempFile::new().unwrap();
    f.as_file().write_all(b"foo").unwrap();
    f.as_file().flush().unwrap();

    let expected = blake3::hash(b"foo").to_hex();
    let output = cmd!(b3sum_exe(), "--no-mmap", "--no-names", f.path())
        .read()
        .unwrap();
    assert_eq!(&*expected, &*output);
}

#[test]
fn test_length_without_value_is_an_error() {
    let result = cmd!(b3sum_exe(), "--length")
        .stdin_bytes("foo")
        .stderr_capture()
        .run();
    assert!(result.is_err());
}

#[test]
fn test_raw_with_multi_files_is_an_error() {
    let f1 = tempfile::NamedTempFile::new().unwrap();
    let f2 = tempfile::NamedTempFile::new().unwrap();

    // Make sure it doesn't error with just one file
    let result = cmd!(b3sum_exe(), "--raw", f1.path()).stdout_capture().run();
    assert!(result.is_ok());

    // Make sure it errors when both file are passed
    let result = cmd!(b3sum_exe(), "--raw", f1.path(), f2.path())
        .stderr_capture()
        .run();
    assert!(result.is_err());
}

#[test]
#[cfg(unix)]
fn test_newline_and_backslash_escaping_on_unix() {
    let empty_hash = blake3::hash(b"").to_hex();
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("subdir")).unwrap();
    let names = [
        "abcdef",
        "abc\ndef",
        "abc\\def",
        "abc\rdef",
        "abc\r\ndef",
        "subdir/foo",
    ];
    let mut paths = Vec::new();
    for name in &names {
        let path = dir.path().join(name);
        println!("creating file at {:?}", path);
        fs::write(&path, b"").unwrap();
        paths.push(path);
    }
    let output = cmd(b3sum_exe(), &names).dir(dir.path()).read().unwrap();
    let expected = format!(
        "\
{0}  abcdef
\\{0}  abc\\ndef
\\{0}  abc\\\\def
\\{0}  abc\\rdef
\\{0}  abc\\r\\ndef
{0}  subdir/foo",
        empty_hash,
    );
    println!("output");
    println!("======");
    println!("{}", output);
    println!();
    println!("expected");
    println!("========");
    println!("{}", expected);
    println!();
    assert_eq!(expected, output);
}

#[test]
#[cfg(windows)]
fn test_slash_normalization_on_windows() {
    let empty_hash = blake3::hash(b"").to_hex();
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("subdir")).unwrap();
    // Note that filenames can't contain newlines or backslashes on Windows, so
    // we don't test escaping here. We only test forward slash and backslash as
    // directory separators.
    let names = ["abcdef", "subdir/foo", "subdir\\bar"];
    let mut paths = Vec::new();
    for name in &names {
        let path = dir.path().join(name);
        println!("creating file at {:?}", path);
        fs::write(&path, b"").unwrap();
        paths.push(path);
    }
    let output = cmd(b3sum_exe(), &names).dir(dir.path()).read().unwrap();
    let expected = format!(
        "\
{0}  abcdef
{0}  subdir/foo
{0}  subdir/bar",
        empty_hash,
    );
    println!("output");
    println!("======");
    println!("{}", output);
    println!();
    println!("expected");
    println!("========");
    println!("{}", expected);
    println!();
    assert_eq!(expected, output);
}

#[test]
#[cfg(unix)]
fn test_invalid_unicode_on_unix() {
    use std::os::unix::ffi::OsStringExt;

    let empty_hash = blake3::hash(b"").to_hex();
    let dir = tempfile::tempdir().unwrap();
    let names = ["abcdef".into(), OsString::from_vec(b"abc\xffdef".to_vec())];
    let mut paths = Vec::new();
    for name in &names {
        let path = dir.path().join(name);
        println!("creating file at {:?}", path);
        // Note: Some operating systems, macOS in particular, simply don't
        // allow invalid Unicode in filenames. On those systems, this write
        // will fail. That's fine, we'll just short-circuit this test in that
        // case. But assert that at least Linux allows this.
        let write_result = fs::write(&path, b"");
        if cfg!(target_os = "linux") {
            write_result.expect("Linux should allow invalid Unicode");
        } else if write_result.is_err() {
            return;
        }
        paths.push(path);
    }
    let output = cmd(b3sum_exe(), &names).dir(dir.path()).read().unwrap();
    let expected = format!(
        "\
{0}  abcdef
{0}  abc�def",
        empty_hash,
    );
    println!("output");
    println!("======");
    println!("{}", output);
    println!();
    println!("expected");
    println!("========");
    println!("{}", expected);
    println!();
    assert_eq!(expected, output);
}

#[test]
#[cfg(windows)]
fn test_invalid_unicode_on_windows() {
    use std::os::windows::ffi::OsStringExt;

    let empty_hash = blake3::hash(b"").to_hex();
    let dir = tempfile::tempdir().unwrap();
    let surrogate_char = 0xDC00;
    let bad_unicode_wchars = [
        'a' as u16,
        'b' as u16,
        'c' as u16,
        surrogate_char,
        'd' as u16,
        'e' as u16,
        'f' as u16,
    ];
    let bad_osstring = OsString::from_wide(&bad_unicode_wchars);
    let names = ["abcdef".into(), bad_osstring];
    let mut paths = Vec::new();
    for name in &names {
        let path = dir.path().join(name);
        println!("creating file at {:?}", path);
        fs::write(&path, b"").unwrap();
        paths.push(path);
    }
    let output = cmd(b3sum_exe(), &names).dir(dir.path()).read().unwrap();
    let expected = format!(
        "\
{0}  abcdef
{0}  abc�def",
        empty_hash,
    );
    println!("output");
    println!("======");
    println!("{}", output);
    println!();
    println!("expected");
    println!("========");
    println!("{}", expected);
    println!();
    assert_eq!(expected, output);
}

#[test]
fn test_check() {
    // Make a directory full of files, and make sure the b3sum output in that
    // directory is what we expect.
    let a_hash = blake3::hash(b"a").to_hex();
    let b_hash = blake3::hash(b"b").to_hex();
    let cd_hash = blake3::hash(b"cd").to_hex();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a"), b"a").unwrap();
    fs::write(dir.path().join("b"), b"b").unwrap();
    fs::create_dir(dir.path().join("c")).unwrap();
    fs::write(dir.path().join("c/d"), b"cd").unwrap();
    let output = cmd!(b3sum_exe(), "a", "b", "c/d")
        .dir(dir.path())
        .stdout_capture()
        .stderr_capture()
        .run()
        .unwrap();
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    let expected_checkfile = format!(
        "{}  a\n\
         {}  b\n\
         {}  c/d\n",
        a_hash, b_hash, cd_hash,
    );
    assert_eq!(expected_checkfile, stdout);
    assert_eq!("", stderr);

    // Now use the output we just validated as a checkfile, passed to stdin.
    let output = cmd!(b3sum_exe(), "--check")
        .stdin_bytes(expected_checkfile.as_bytes())
        .dir(dir.path())
        .stdout_capture()
        .stderr_capture()
        .run()
        .unwrap();
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    let expected_check_output = "\
         a: OK\n\
         b: OK\n\
         c/d: OK\n";
    assert_eq!(expected_check_output, stdout);
    assert_eq!("", stderr);

    // Now pass the same checkfile twice on the command line just for fun.
    let checkfile_path = dir.path().join("checkfile");
    fs::write(&checkfile_path, &expected_checkfile).unwrap();
    let output = cmd!(b3sum_exe(), "--check", &checkfile_path, &checkfile_path)
        .dir(dir.path())
        .stdout_capture()
        .stderr_capture()
        .run()
        .unwrap();
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    let mut double_check_output = String::new();
    double_check_output.push_str(&expected_check_output);
    double_check_output.push_str(&expected_check_output);
    assert_eq!(double_check_output, stdout);
    assert_eq!("", stderr);

    // Corrupt one of the files and check again.
    fs::write(dir.path().join("b"), b"CORRUPTION").unwrap();
    let output = cmd!(b3sum_exe(), "--check", &checkfile_path)
        .dir(dir.path())
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .unwrap();
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    let expected_check_failure = "\
        a: OK\n\
        b: FAILED\n\
        c/d: OK\n";
    assert!(!output.status.success());
    assert_eq!(expected_check_failure, stdout);
    assert_eq!(
        "b3sum: WARNING: 1 computed checksum did NOT match\n",
        stderr,
    );

    // Delete one of the files and check again.
    fs::remove_file(dir.path().join("b")).unwrap();
    let open_file_error = fs::File::open(dir.path().join("b")).unwrap_err();
    let output = cmd!(b3sum_exe(), "--check", &checkfile_path)
        .dir(dir.path())
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .unwrap();
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    let expected_check_failure = format!(
        "a: OK\n\
         b: FAILED ({})\n\
         c/d: OK\n",
        open_file_error,
    );
    assert!(!output.status.success());
    assert_eq!(expected_check_failure, stdout);
    assert_eq!(
        "b3sum: WARNING: 1 computed checksum did NOT match\n",
        stderr,
    );

    // Confirm that --quiet suppresses the OKs but not the FAILEDs.
    let output = cmd!(b3sum_exe(), "--check", "--quiet", &checkfile_path)
        .dir(dir.path())
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .unwrap();
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    let expected_check_failure = format!("b: FAILED ({})\n", open_file_error);
    assert!(!output.status.success());
    assert_eq!(expected_check_failure, stdout);
    assert_eq!(
        "b3sum: WARNING: 1 computed checksum did NOT match\n",
        stderr,
    );
}

#[test]
fn test_check_invalid_characters() {
    // Check that a null character in the path fails.
    let output = cmd!(b3sum_exe(), "--check")
        .stdin_bytes("0000000000000000000000000000000000000000000000000000000000000000  \0")
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .unwrap();
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    let expected_stderr = "\
        b3sum: Null character in path\n\
        b3sum: WARNING: 1 computed checksum did NOT match\n";
    assert!(!output.status.success());
    assert_eq!("", stdout);
    assert_eq!(expected_stderr, stderr);

    // Check that a Unicode replacement character in the path fails.
    let output = cmd!(b3sum_exe(), "--check")
        .stdin_bytes("0000000000000000000000000000000000000000000000000000000000000000  �")
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .unwrap();
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    let expected_stderr = "\
        b3sum: Unicode replacement character in path\n\
        b3sum: WARNING: 1 computed checksum did NOT match\n";
    assert!(!output.status.success());
    assert_eq!("", stdout);
    assert_eq!(expected_stderr, stderr);

    // Check that an invalid escape sequence in the path fails.
    let output = cmd!(b3sum_exe(), "--check")
        .stdin_bytes("\\0000000000000000000000000000000000000000000000000000000000000000  \\a")
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .unwrap();
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    let expected_stderr = "\
        b3sum: Invalid backslash escape\n\
        b3sum: WARNING: 1 computed checksum did NOT match\n";
    assert!(!output.status.success());
    assert_eq!("", stdout);
    assert_eq!(expected_stderr, stderr);

    // Windows also forbids literal backslashes. Check for that if and only if
    // we're on Windows.
    if cfg!(windows) {
        let output = cmd!(b3sum_exe(), "--check")
            .stdin_bytes("0000000000000000000000000000000000000000000000000000000000000000  \\")
            .stdout_capture()
            .stderr_capture()
            .unchecked()
            .run()
            .unwrap();
        let stdout = std::str::from_utf8(&output.stdout).unwrap();
        let stderr = std::str::from_utf8(&output.stderr).unwrap();
        let expected_stderr = "\
            b3sum: Backslash in path\n\
            b3sum: WARNING: 1 computed checksum did NOT match\n";
        assert!(!output.status.success());
        assert_eq!("", stdout);
        assert_eq!(expected_stderr, stderr);
    }
}

#[test]
fn test_globbing() {
    // On Unix, globbing is provided by the shell. On Windows, globbing is
    // provided by us, using the `wild` crate.
    let dir = tempfile::tempdir().unwrap();
    let file1 = dir.path().join("file1");
    fs::write(&file1, b"foo").unwrap();
    let file2 = dir.path().join("file2");
    fs::write(&file2, b"bar").unwrap();

    let foo_hash = blake3::hash(b"foo");
    let bar_hash = blake3::hash(b"bar");
    // NOTE: This assumes that the glob will be expanded in alphabetical order,
    //       to "file1 file2" rather than "file2 file1". So far, this seems to
    //       be true (guaranteed?) of Unix shell behavior, and true in practice
    //       with the `wild` crate on Windows. It's possible that this could
    //       start failing in the future, though, or on some unknown platform.
    //       If that ever happens, we'll need to relax this test somehow,
    //       probably by just testing for both possible outputs. I'm not
    //       handling that case in advance, though, because I'd prefer to hear
    //       about it if it comes up.
    let expected = format!("{}  file1\n{}  file2", foo_hash.to_hex(), bar_hash.to_hex());

    let star_command = format!("{} *", b3sum_exe().to_str().unwrap());
    let (exe, c_flag) = if cfg!(windows) {
        ("cmd.exe", "/C")
    } else {
        ("/bin/sh", "-c")
    };
    let output = cmd!(exe, c_flag, star_command)
        .dir(dir.path())
        .read()
        .unwrap();
    assert_eq!(expected, output);
}
