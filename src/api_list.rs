//! The API list -sdk_imports_api_list reads: a JSON object whose
//! "version" goes into the -sdk_imports report and whose "apis" are the
//! only imports the report lists.

use crate::error::RawPath;
use std::path::Path;

use serde_json::Value;

/// An API list: its version, and the symbol names it lists.
#[derive(Clone, Debug, Default)]
pub struct ApiList {
    pub version: i32,
    pub apis: hashbrown::HashSet<Vec<u8>>,
}

/// Reads an API list: its version, an integer (or a string of one), and
/// the strings of its "apis" array, of which there must be one at least.
pub fn read(path: &Path) -> ApiList {
    let fail = |what: &dyn std::fmt::Display| -> ! {
        crate::fatal!("-sdk_imports_api_list invalid list at {}: {what}", path.raw());
    };
    let data = std::fs::read(path).unwrap_or_else(|e| fail(&crate::error::strerror(&e)));
    let root: Value = serde_json::from_slice(&data).unwrap_or_else(|e| fail(&e));
    let version = match &root["version"] {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    };
    let Some(version) = version.and_then(|v| i32::try_from(v).ok()) else {
        fail(&"no version");
    };
    let apis: hashbrown::HashSet<Vec<u8>> = (root["apis"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(|name| name.as_bytes().to_vec())
        .collect();
    if apis.is_empty() {
        fail(&"no APIs listed");
    }
    ApiList { version, apis }
}
