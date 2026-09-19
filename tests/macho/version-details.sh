#!/bin/bash
source "$(dirname "$0")"/common.inc

# `ld -v` alone prints the banner and exits 0; build systems probe the
# linker that way.
$mold -v > $t/v.txt
grep -q '[ms]old' $t/v.txt

# Xcode runs `ld -version_details` before the first link and needs JSON
# with an ld64 "version" and the supported "architectures".
$mold -version_details > $t/details.json
python3 - $t/details.json <<'EOF'
import json, sys
d = json.load(open(sys.argv[1]))
assert d["version"].isdigit(), d
assert "arm64" in d["architectures"] and "x86_64" in d["architectures"], d
EOF
