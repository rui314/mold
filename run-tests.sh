#!/usr/bin/env bash
# Compatibility entry point for the Rust test runner. The test cases remain
# the unchanged upstream shell scripts under test/.

set -e
cd "$(dirname "$0")"
exec cargo run --quiet --release -p mold-test-runner -- "$@"
