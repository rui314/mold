#! /usr/bin/env bash

# This hacky script works around the fact that `cross test` does not support
# path dependencies. (It uses a docker shared folder to let the guest access
# project files, so parent directories aren't available.) Solve this problem by
# copying the entire project to a temp dir and rearranging paths to put "c" and
# "reference_impl" underneath "blake3_c_rust_bindings", so that everything is
# accessible. Hopefully this will just run on CI forever and no one will ever
# read this and discover my deep shame.

set -e -u -o pipefail

project_root="$(realpath "$(dirname "$BASH_SOURCE")/../..")"
tmpdir="$(mktemp -d)"
echo "Running cross tests in $tmpdir"
cd "$tmpdir"
git clone "$project_root" blake3
mv blake3/c/blake3_c_rust_bindings .
mv blake3/reference_impl blake3_c_rust_bindings
mv blake3/c blake3_c_rust_bindings
cd blake3_c_rust_bindings
sed -i 's|reference_impl = { path = "../../reference_impl" }|reference_impl = { path = "reference_impl" }|' Cargo.toml

export BLAKE3_C_DIR_OVERRIDE="./c"
cat > Cross.toml << EOF
[build.env]
passthrough = [
    "BLAKE3_C_DIR_OVERRIDE",
]
EOF
cross test "$@"
