#!/bin/bash
source "$(dirname "$0")"/common.inc

# The dyld shared cache builder refuses a library with interposing
# tuples, so ld-prime rejects them in an image bound for the shared
# region: a section named __interpose in a segment whose name starts
# with __DATA or __AUTH, by the section's final name, even an empty
# one. -not_for_dyld_shared_cache opts out, and an image bound for no
# shared region isn't checked.
cat <<'EOF' | $CC -o $t/a.o -c -xc -
#include <stdio.h>
static int my_puts(const char *s) { return printf("interposed %s\n", s); }
__attribute__((used, section("__DATA,__interpose"))) static struct {
  void *replacement, *replacee;
} tuples[] = { { (void *)my_puts, (void *)puts } };
EOF
echo 'int foo(void) { return 1; }' | $CC -o $t/b.o -c -xc -
printf '%016d' 0 > $t/tuples

msg() {
  echo "Shared cache eligible dylib cannot use interposing tuples (found in '$1 __interpose').  Remove interposing tuples, or opt out of the shared cache using the build setting 'LD_SHARED_CACHE_ELIGIBLE=NO' (or linker flag '-not_for_dyld_shared_cache')"
}

not $CC --ld-path=$mold -o $t/a.dylib -shared $t/a.o -mmacosx-version-min=15.0 \
  -Wl,-install_name,/usr/lib/libfoo.dylib 2> $t/log
# (Before iOS 18 too, an old simulator's, the tuples stay in __DATA.)
if simulator_older_than 18; then seg=__DATA; else seg=__DATA_CONST; fi
grep -qF "$(msg $seg)" $t/log

# (A simulator's objects are built for its version, whatever macOS's.)
if ! on_simulator; then
  not $CC --ld-path=$mold -o $t/b.dylib -shared $t/a.o -mmacosx-version-min=14.0 \
    -Wl,-install_name,/System/Library/Frameworks/Foo.framework/Foo 2> $t/log2
  grep -qF "$(msg __DATA)" $t/log2
fi

not $CC --ld-path=$mold -o $t/c.dylib -shared $t/b.o -Wl,-install_name,/usr/lib/libfoo.dylib \
  -Wl,-add_empty_section,__DATA,__interpose 2> $t/log3
grep -q 'cannot use interposing tuples' $t/log3

not $CC --ld-path=$mold -o $t/d.dylib -shared $t/b.o -Wl,-install_name,/usr/lib/libfoo.dylib \
  -Wl,-sectcreate,__AUTH,__interpose,$t/tuples 2> $t/log4
grep -qF "$(msg __AUTH)" $t/log4

# Of several, ld-prime names the last in the image.
not $CC --ld-path=$mold -o $t/d.dylib -shared $t/b.o -Wl,-install_name,/usr/lib/libfoo.dylib \
  -Wl,-sectcreate,__DATA_ZZZ,__interpose,$t/tuples \
  -Wl,-sectcreate,__DATA_AAA,__interpose,$t/tuples 2> $t/log4
grep -qF "$(msg __DATA_AAA)" $t/log4

not $CC --ld-path=$mold -o $t/e.bundle -bundle $t/a.o -Wl,-add_split_seg_info 2> $t/log5
grep -q 'cannot use interposing tuples' $t/log5

$CC --ld-path=$mold -o $t/f.dylib -shared $t/a.o -Wl,-install_name,/usr/lib/libfoo.dylib \
  -Wl,-not_for_dyld_shared_cache
$CC --ld-path=$mold -o $t/g.dylib -shared $t/a.o -Wl,-install_name,/usr/local/lib/libfoo.dylib
$CC --ld-path=$mold -o $t/h.dylib -shared $t/a.o -Wl,-install_name,/usr/lib/libfoo.dylib \
  -Wl,-rename_section,__DATA,__interpose,__DATA,__bar
$CC --ld-path=$mold -o $t/i.dylib -shared $t/b.o -Wl,-install_name,/usr/lib/libfoo.dylib \
  -Wl,-sectcreate,__FOO,__interpose,$t/tuples

# An arm64 kext is bound for a kernel collection's shared region.
if [ $ARCH = arm64 ]; then
  not $mold -arch $ARCH -kext $t/b.o -o $t/kext -sectcreate __DATA __interpose $t/tuples \
    2> $t/log6
  grep -qF "$(msg __DATA)" $t/log6
fi
