#!/bin/bash
source "$(dirname "$0")"/common.inc

# `ld -v` alone prints the banner (on stderr) and exits 0; build
# systems probe the linker that way.
banner='mold-macho\|PROGRAM:ld'
$mold -v 2> $t/v.txt
grep "$banner" $t/v.txt

# Xcode runs `ld -version_details` before the first link and needs JSON
# with an ld64 "version" and the supported "architectures".
check_json() {
  python3 - $1 <<'EOF'
import json, re, sys
d = json.load(open(sys.argv[1]))
assert re.fullmatch(r"\d+(\.\d+)*", d["version"]), d
assert "arm64" in d["architectures"] and "x86_64" in d["architectures"], d
EOF
}
$mold -version_details > $t/details.json
check_json $t/details.json

# With nothing to link, -v wins: the banner alone, wherever
# -version_details stands.
$mold -version_details -v > $t/out2 2> $t/err2
[ ! -s $t/out2 ]
grep "$banner" $t/err2

# With something to link, -version_details prints the JSON and, as -v
# does, the search paths (but no banner), then links.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
$CC --ld-path=$mold -o $t/exe $t/a.o -Wl,-version_details > $t/details3.json 2> $t/err3
check_json $t/details3.json
grep '^Library search paths:' $t/err3
not grep "$banner" $t/err3
$t/exe
