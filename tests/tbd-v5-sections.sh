#!/bin/bash
source "$(dirname "$0")"/common.inc

# TAPI reads a version 5 (JSON) .tbd section by section, and ld-prime
# refuses one whose sections it can't read, naming the first such:
# "tapi error: invalid <key> section". It ignores keys it doesn't know.
# Of the install names and the versions, it reads the first entry
# only, whatever its targets; a group whose targets aren't all strings
# applies to every target.
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo();
int main() { return foo(); }
EOF

# check <key> <main_library fields>
check() {
  cat > $t/lib.tbd <<EOF
{ "tapi_tbd_version": 5, "main_library": { $2 } }
EOF
  not $CC --ld-path=$mold -o $t/exe $t/a.o $t/lib.tbd 2> $t/log
  grep -q "tapi error: invalid $1 section$" $t/log
  grep -q "^ in '$t/lib.tbd'$" $t/log
}

name='"install_names": [{"name": "/usr/lib/libt.dylib"}]'
target='"target_info": [{"target": "'$ARCH'-macos"}]'
syms='"exported_symbols": [{"text": {"global": ["_foo"]}}]'

check targets "$name, $syms"
check target "$name, $syms, \"target_info\": [{\"target\": 1}]"
check min_deployment "$name, $syms, \"target_info\": [{\"target\": \"$ARCH-macos\", \"min_deployment\": \"1.\"}]"
check install_names "$target, $syms"
check name "$target, $syms, \"install_names\": [{\"nam\": \"/usr/lib/libt.dylib\"}]"
check version "$name, $target, $syms, \"current_versions\": [{\"version\": \"1.256\"}]"
check abi "$name, $target, $syms, \"swift_abi\": [{\"abi\": \"5\"}]"
check attributes "$name, $target, $syms, \"flags\": [{\"attributes\": [1]}]"
check umbrella "$name, $target, $syms, \"parent_umbrellas\": [{\"umb\": \"Foo\"}]"
check clients "$name, $target, $syms, \"allowable_clients\": [{\"clients\": [1]}]"
check exported_symbols "$name, $target, \"exported_symbols\": [{\"txt\": {}}]"
check weak "$name, $target, \"exported_symbols\": [{\"text\": {\"global\": [\"_foo\"], \"weak\": [1]}}]"
# The targets come first, the versions before the symbols.
check target "\"install_names\": [{}], $syms, \"target_info\": [{}]"
check version "$name, $target, \"exported_symbols\": [{}], \"current_versions\": [{\"version\": \"x\"}]"

cat > $t/lib.tbd <<EOF
{
  "tapi_tbd_version": 5,
  "unknown": 1,
  "main_library": {
    "target_info": [{"target": "$ARCH-macos"}, {"target": "arm64e-macos"}],
    "install_names": [{"targets": ["arm64e-macos"], "name": "/usr/lib/libfirst.dylib"},
                      {"name": "/usr/lib/libsecond.dylib"}],
    "current_versions": [{"targets": ["arm64e-macos"], "version": "2"}, {"version": "3"}],
    "exported_symbols": [{"targets": ["arm64e-macos", 1], "text": {"global": ["_foo"]}}]
  }
}
EOF
$CC --ld-path=$mold -o $t/exe $t/a.o $t/lib.tbd
otool -L $t/exe | grep -q '/usr/lib/libfirst.dylib (compatibility version 1.0.0, current version 2.0.0)'
