#!/bin/bash
source "$(dirname "$0")"/common.inc

# -warn_weak_exports names, by name, each weak definition the image
# exports and each definition overriding a dylib's weak one, which dyld
# must coalesce at launch; -no_weak_exports refuses an image with any.
cat <<EOF | $CC -o $t/a.o -c -xc -
__attribute__((weak)) int foo() { return 1; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
int foo() { return 5; }
__attribute__((weak)) int zweak() { return 1; }
__attribute__((weak)) int aweak() { return 1; }
__attribute__((weak, visibility("hidden"))) int hidden() { return 1; }
int main() { return foo() + aweak() + zweak() + hidden(); }
EOF

$CC --ld-path=$mold -o $t/liba.dylib -shared $t/a.o
$CC --ld-path=$mold -o $t/exe $t/b.o $t/liba.dylib -Wl,-warn_weak_exports 2> $t/log
cat > $t/expected <<EOF
warning: weak external symbol: _aweak
warning: overrides weak external symbol: _foo
warning: weak external symbol: _zweak
EOF
sed 's/^[a-z]*: //' $t/log | diff - $t/expected

not $CC --ld-path=$mold -o $t/exe $t/b.o $t/liba.dylib -Wl,-no_weak_exports 2> $t/log
grep -q 'output has external weak-def symbols, but -no_weak_exports used' $t/log

# Nothing weak left to export is fine.
$CC --ld-path=$mold -o $t/libb.dylib -shared $t/b.o -Wl,-no_weak_exports \
  -Wl,-exported_symbol,_main 2> $t/log
not grep -q . $t/log

# An object file is only warned about.
$mold -r -o $t/c.o $t/b.o -warn_weak_exports -no_weak_exports 2> $t/log
grep -q 'weak external symbol: _zweak' $t/log
not grep -q 'no_weak_exports used' $t/log
