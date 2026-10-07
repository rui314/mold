#!/bin/bash
source "$(dirname "$0")"/common.inc

# -map lists the files of the link: the objects in link order - an
# archive's member as "lib.a(member.o)", wherever an auto-link option
# loaded it from -, then the dylibs the output links, each once, those
# auto-link options load too. (ld-prime lists the files in the order it
# loads them, dylibs among the objects, and also each naming of a dylib
# and the dylibs -dead_strip_dylibs drops.)
tbd() {
  cat <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
install-name:    '$1'
current-version: 1
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ $2 ]
...
EOF
}
tbd /aaa/libaaa.dylib _aaa > $t/libaaa.tbd
tbd /zzz/libzzz.dylib _zzz > $t/libzzz.tbd
echo 'int mid(void) { return 0; }' | $CC -o $t/mid.o -c -xc -
echo 'int arc(void) { return 0; }' | $CC -o $t/arc.o -c -xc -
rm -f $t/libmid.a $t/libarc.a
ar rcs $t/libmid.a $t/mid.o
ar rcs $t/libarc.a $t/arc.o

cat <<'EOF' | $CC -o $t/a.o -c -xc -
__asm__(".linker_option \"-lzzz\"");
__asm__(".linker_option \"-lmid\"");
void aaa(void), zzz(void);
int mid(void), arc(void);
int main() { aaa(); zzz(); return mid() + arc(); }
EOF
echo 'int b = 1;' | $CC -o $t/b.o -c -xc -

$CC --ld-path=$mold -o $t/exe $t/a.o $t/libarc.a $t/b.o $t/libaaa.tbd -L$t -Wl,-map,$t/map
sed -n '/^# Object files:/,/^# Sections:/p' $t/map | grep '^\[' > $t/files
diff - $t/files <<EOF
[  0] linker synthesized
[  1] $t/a.o
[  2] $t/libarc.a(arc.o)
[  3] $t/b.o
[  4] $t/libmid.a(mid.o)
[  5] $t/libaaa.tbd
[  6] $SDK/usr/lib/libSystem.tbd
[  7] $t/libzzz.tbd
EOF

# A dylib named twice is one file; one -dead_strip_dylibs drops is
# none.
echo 'int foo(void) { return 1; }' | $CC -shared -xc - -o $t/libfoo.dylib
echo 'int bar(void) { return 1; }' | $CC -shared -xc - -o $t/libbar.dylib
echo 'int foo(void); int main() { return foo(); }' | $CC -o $t/e.o -c -xc -
$CC --ld-path=$mold -o $t/exe2 $t/e.o $t/libfoo.dylib $t/libbar.dylib $t/libfoo.dylib \
  -Wl,-dead_strip_dylibs -Wl,-map,$t/map2
sed -n '/^# Object files:/,/^# Sections:/p' $t/map2 > $t/files2
[ "$(grep -c "\] $t/libfoo.dylib$" $t/files2)" -eq 1 ]
not grep -q libbar $t/files2
