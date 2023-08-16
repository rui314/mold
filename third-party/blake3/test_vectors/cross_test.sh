#! /usr/bin/env bash

# This hacky script works around the fact that `cross test` does not support
# path dependencies. (It uses a docker shared folder to let the guest access
# project files, so parent directories aren't available.) Solve this problem by
# copying the entire project to a temp dir and rearranging paths to put
# "blake3" and "reference_impl" underneath "test_vectors", so that everything
# is accessible. Hopefully this will just run on CI forever and no one will
# ever read this and discover my deep shame.

set -e -u -o pipefail

project_root="$(realpath "$(dirname "$BASH_SOURCE")/..")"
tmpdir="$(mktemp -d)"
echo "Running cross tests in $tmpdir"
cd "$tmpdir"
git clone "$project_root" blake3
mv blake3/test_vectors .
mv blake3/reference_impl test_vectors
mv blake3 test_vectors
cd test_vectors
sed -i 's|blake3 = { path = "../" }|blake3 = { path = "./blake3" }|' Cargo.toml
sed -i 's|reference_impl = { path = "../reference_impl" }|reference_impl = { path = "reference_impl" }|' Cargo.toml

cross test "$@"
