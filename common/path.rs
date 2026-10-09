//! Lexical path manipulation, which doesn't consult the file system.

/// Normalizes a path lexically, resolving `.` and `..` components without
/// consulting the file system.
pub fn path_clean(path: &str) -> String {
    clean_path(std::path::Path::new(path)).to_string_lossy().into_owned()
}

/// Normalizes an OS path without resolving symlinks.
pub fn clean_path(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::{Component, PathBuf};
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::RootDir) => {}
                None | Some(Component::ParentDir) => out.push(".."),
                _ => {
                    out.pop();
                }
            },
            other => out.push(other),
        }
    }
    if out.as_os_str().is_empty() { PathBuf::from(".") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_paths() {
        assert_eq!(path_clean("a/./b/../c"), "a/c");
        assert_eq!(path_clean("/a/../.."), "/");
        assert_eq!(path_clean("../a"), "../a");
        assert_eq!(path_clean("../../a/b"), "../../a/b");
        assert_eq!(path_clean("a/../../b"), "../b");
        assert_eq!(path_clean("a/b/../../../c"), "../c");
        assert_eq!(path_clean(".."), "..");
        assert_eq!(path_clean("/.."), "/");
    }
}
