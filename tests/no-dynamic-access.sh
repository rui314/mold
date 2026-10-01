#!/bin/bash
source "$(dirname "$0")"/common.inc

# -no_dynamic_access marks a dylib or a main executable MH_NOFIXPREBINDING
# (0x400): dyld neither dlopen()s it nor finds its symbols with dlsym().
cat <<EOF | $CC -o $t/a.o -c -xc -
int foo() { return 3; }
EOF

cat <<EOF | $CC -o $t/b.o -c -xc -
#include <dlfcn.h>
#include <stdio.h>
int main(int argc, char **argv) {
  void *h = dlopen(argv[1], RTLD_NOW);
  printf("%s\n", h ? "loaded" : "refused");
}
EOF

$CC --ld-path=$mold -o $t/libfoo.dylib -shared $t/a.o -Wl,-no_dynamic_access
otool -hv $t/libfoo.dylib | grep -q NOFIXPREBINDING
$CC --ld-path=$mold -o $t/libbar.dylib -shared $t/a.o
otool -hv $t/libbar.dylib > $t/flags
not grep -q NOFIXPREBINDING $t/flags

$CC --ld-path=$mold -o $t/exe $t/b.o -Wl,-no_dynamic_access
otool -hv $t/exe | grep -q NOFIXPREBINDING
if native_arch; then
  $t/exe $t/libfoo.dylib | grep -q '^refused$'
  $t/exe $t/libbar.dylib | grep -q '^loaded$'
fi

# Elsewhere the option is ignored with a warning.
$CC --ld-path=$mold -o $t/c.bundle -bundle $t/a.o -Wl,-no_dynamic_access 2> $t/log
grep -q 'warning: -no_dynamic_access ignored. It can only be used with dylibs and main executables' $t/log
otool -hv $t/c.bundle > $t/flags2
not grep -q NOFIXPREBINDING $t/flags2
