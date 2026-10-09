#!/bin/bash
source "$(dirname "$0")"/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
void foo() {}
EOF

mkdir -p $t/x/y

$CC --ld-path=$mold -shared -o $t/x/y/libfoo.dylib $t/a.o -Wl,-install_name,@executable_path/x/y/libfoo.dylib

cat <<EOF | $CC -o $t/b.o -c -xc -
void bar() {}
EOF

$CC --ld-path=$mold -shared -o $t/libbar.dylib $t/b.o -Wl,-reexport_library,$t/x/y/libfoo.dylib

objdump --macho --dylibs-used $t/libbar.dylib | grep 'libfoo.*reexport'

cat <<EOF | $CC -o $t/d.o -c -xc -
void foo();
void bar();

int main() {
  foo();
  bar();
}
EOF

# ld-prime expands no @executable_path in a re-exported library's
# install name (ld64 took the output's directory for it): the library
# is found by its leaf in the library search path, or not at all.
not $CC --ld-path=$mold -o $t/exe $t/d.o -L$t -lbar 2> $t/log
grep -q "ignoring missing indirect library: library for install name '@executable_path/x/y/libfoo.dylib' not found" $t/log

$CC --ld-path=$mold -o $t/exe $t/d.o -L$t -L$t/x/y -lbar
$RUN $t/exe

# -executable_path, which ld64 took for the output's path, is obsolete:
# ignored with a warning.
not $CC --ld-path=$mold -o $t/exe $t/d.o -L$t -lbar -Wl,-executable_path,$t/exe 2> $t/log
grep -q -- '-executable_path is obsolete' $t/log
$CC --ld-path=$mold -o $t/exe $t/d.o -L$t -L$t/x/y -lbar -Wl,-w \
  -Wl,-executable_path,$t/exe 2> $t/log
not grep -q -- '-executable_path' $t/log
