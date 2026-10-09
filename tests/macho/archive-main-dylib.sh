#!/bin/bash
source "$(dirname "$0")"/common.inc

# An executable's entry point pulls the archive member that defines it,
# but a dylib or a bundle has none: a member defining _main that nothing
# else needs stays out of one (Lua's lua.o out of Hammerspoon's LuaSkin).
cat <<EOF | $CC -o $t/a.o -c -xc -
int lib_fn(void);
int use(void) { return lib_fn(); }
EOF
echo 'int lib_fn(void) { return 1; }' | $CC -o $t/b.o -c -xc -
echo 'int main_helper = 5; int main() { return 0; }' | $CC -o $t/c.o -c -xc -
rm -f $t/libx.a
ar rcs $t/libx.a $t/b.o $t/c.o

$CC --ld-path=$mold -shared -o $t/d.dylib $t/a.o $t/libx.a
nm $t/d.dylib > $t/log1
not grep -q _main_helper $t/log1
$CC --ld-path=$mold -bundle -o $t/e.bundle $t/a.o $t/libx.a
nm $t/e.bundle > $t/log2
not grep -q _main_helper $t/log2
$CC --ld-path=$mold -o $t/exe $t/a.o $t/libx.a
nm $t/exe > $t/log3
grep -q _main_helper $t/log3
