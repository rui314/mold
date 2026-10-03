#!/bin/bash
source "$(dirname "$0")"/common.inc

# -init names a function the image runs before its other initializers:
# ld-prime makes it the first of the image's initializer offsets, in an
# executable, a bundle or a dylib alike. It is an initial undefine, as
# if -u named it, and the last -init given wins.
cat <<EOF | $CC -o $t/a.o -c -xc - -mmacosx-version-min=11.0
#include <stdio.h>
__attribute__((visibility("hidden"))) void init(void) { printf("init "); }
__attribute__((constructor)) void ctor(void) { printf("ctor "); }
EOF

cat <<EOF | $CC -o $t/main.o -c -xc - -mmacosx-version-min=11.0
#include <dlfcn.h>
#include <stdio.h>
int main(int argc, char **argv) {
  printf("main ");
  printf("%d\n", argc < 2 || dlopen(argv[1], RTLD_NOW) != 0);
}
EOF

$CC --ld-path=$mold -o $t/exe $t/main.o $t/a.o -Wl,-init,_nosuch -Wl,-init,_init
[ "$($t/exe)" = 'init ctor main 1' ]

$CC --ld-path=$mold -o $t/exe2 $t/main.o
$CC --ld-path=$mold -o $t/lib.dylib -shared $t/a.o -Wl,-init,_init -Wl,-dead_strip
[ "$($t/exe2 $t/lib.dylib)" = 'main init ctor 1' ]
otool -l $t/lib.dylib | grep -A4 'sectname __init_offsets' | grep -q 'size 0x0*8$'

# Of an older image, which keeps __mod_init_func, ld-prime runs no
# -init function; ld64 named it in LC_ROUTINES_64, which dyld runs
# before the other initializers, and so do we.
if $mold -v 2>&1 | grep -q mold-macho; then
  $CC --ld-path=$mold -o $t/lib2.dylib -shared $t/a.o -Wl,-init,_init -mmacosx-version-min=11.0
  [ "$($t/exe2 $t/lib2.dylib)" = 'main init ctor 1' ]
  otool -l $t/lib2.dylib | grep -q 'cmd LC_ROUTINES_64'
fi

not $CC --ld-path=$mold -o $t/lib3.dylib -shared $t/a.o -Wl,-init,_nosuch \
  -Wl,-undefined,dynamic_lookup 2> $t/log3
grep -v '^+' $t/log3 | grep -A1 _nosuch | grep -q 'the command line'

# An offset can't reach a function in another image.
not $CC --ld-path=$mold -o $t/lib4.dylib -shared $t/a.o -Wl,-init,_puts 2> $t/log4
grep -q "__init_offsets entry 0: target '_puts' does not have address" $t/log4

# Nor does an absolute symbol. (ld-prime takes the low 32 bits of its
# value for the offset.)
cat <<EOF | $CC -o $t/abs.o -c -xassembler -
.globl _abs
.set _abs, 0x123456789
EOF
not $CC --ld-path=$mold -o $t/lib6.dylib -shared $t/a.o $t/abs.o -Wl,-init,_abs 2> $t/log6
grep -q "'_abs'" $t/log6

$mold -arch $ARCH -r -o $t/r.o $t/a.o -init _nosuch
nm $t/r.o | grep -q ' U _nosuch$'

not $mold -arch $ARCH -dylib -init 2> $t/log5
grep -q -- '-init.*missing' $t/log5
