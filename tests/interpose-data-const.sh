#!/bin/bash
source "$(dirname "$0")"/common.inc

# dyld reads an image's interposing tuples but never writes them, so
# from macOS 15 on ld-prime puts __DATA,__interpose in __DATA_CONST,
# even with -no_data_const; before, it stays in __DATA.
cat <<'EOF' | $CC -o $t/a.o -c -xc -
#include <stdio.h>
static int my_puts(const char *s) { return printf("interposed %s\n", s); }
__attribute__((used, section("__DATA,__interpose"))) static struct {
  void *replacement, *replacee;
} tuples[] = { { (void *)my_puts, (void *)puts } };
EOF
cat <<'EOF' | $CC -o $t/b.o -c -xc -
#include <stdio.h>
int main() { puts("hello"); }
EOF

sects() {
  otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { print $2 "," s; s = "" }'
}

$CC --ld-path=$mold -o $t/exe $t/b.o

$CC --ld-path=$mold -o $t/libfoo.dylib -shared $t/a.o -mmacosx-version-min=15.0
sects $t/libfoo.dylib > $t/sects
grep -qx '__DATA_CONST,__interpose' $t/sects
DYLD_INSERT_LIBRARIES=$t/libfoo.dylib $t/exe > $t/out
grep -qx 'interposed hello' $t/out

$CC --ld-path=$mold -o $t/libfoo2.dylib -shared $t/a.o -mmacosx-version-min=15.0 \
  -Wl,-no_data_const
sects $t/libfoo2.dylib > $t/sects2
grep -qx '__DATA_CONST,__interpose' $t/sects2
not grep -q '__DATA_CONST,__got' $t/sects2
DYLD_INSERT_LIBRARIES=$t/libfoo2.dylib $t/exe > $t/out2
grep -qx 'interposed hello' $t/out2

$CC --ld-path=$mold -o $t/libfoo3.dylib -shared $t/a.o -mmacosx-version-min=14.0
sects $t/libfoo3.dylib > $t/sects3
grep -qx '__DATA,__interpose' $t/sects3
DYLD_INSERT_LIBRARIES=$t/libfoo3.dylib $t/exe > $t/out3
grep -qx 'interposed hello' $t/out3

# The move takes the place of a -rename_section: one naming
# __DATA,__interpose keeps the tuples out of __DATA_CONST, one naming
# __DATA_CONST,__interpose never applies, and -rename_segment moves them
# on from there. A -sectcreate section of that name moves too.
$CC --ld-path=$mold -o $t/libfoo4.dylib -shared $t/a.o -mmacosx-version-min=15.0 \
  -Wl,-rename_section,__DATA,__interpose,__DATA,__bar
sects $t/libfoo4.dylib > $t/sects4
grep -qx '__DATA,__bar' $t/sects4
not grep -q __interpose $t/sects4

$CC --ld-path=$mold -o $t/libfoo5.dylib -shared $t/a.o -mmacosx-version-min=15.0 \
  -Wl,-rename_section,__DATA_CONST,__interpose,__DATA,__bar
sects $t/libfoo5.dylib > $t/sects5
grep -qx '__DATA_CONST,__interpose' $t/sects5

$CC --ld-path=$mold -o $t/libfoo6.dylib -shared $t/a.o -mmacosx-version-min=15.0 \
  -Wl,-rename_segment,__DATA_CONST,__FOO
sects $t/libfoo6.dylib > $t/sects6
grep -qx '__FOO,__interpose' $t/sects6

printf '%016d' 0 > $t/tuples
$CC --ld-path=$mold -o $t/libfoo7.dylib -shared $t/b.o -mmacosx-version-min=15.0 \
  -Wl,-sectcreate,__DATA,__interpose,$t/tuples
sects $t/libfoo7.dylib > $t/sects7
grep -qx '__DATA_CONST,__interpose' $t/sects7
