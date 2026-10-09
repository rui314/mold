#!/bin/bash
source "$(dirname "$0")"/common.inc

# An image may name its own mach header by a symbol of its kind:
# __mh_execute_header in an executable, which it exports, and
# __mh_dylib_header, __mh_bundle_header or __mh_dylinker_header in a
# dylib, a bundle or dyld, which stay out of the symbol table as
# ___dso_handle does. A pointer to one is a rebase to the image's start.
for kind in dylib bundle dylinker; do
  cat <<EOF | $CC -o $t/$kind.o -c -xc -
extern char _mh_${kind}_header[], __dso_handle[];
char *p[] = { _mh_${kind}_header, __dso_handle };
void start(void) __asm__("start");
void start(void) {}
EOF
done

$CC --ld-path=$mold -shared -o $t/libfoo.dylib $t/dylib.o
$CC --ld-path=$mold -bundle -o $t/foo.bundle $t/bundle.o
$mold -arch $ARCH -dylinker -o $t/dyld $t/dylinker.o
for f in libfoo.dylib foo.bundle dyld; do
  [ "$(dyld_info -fixups $t/$f | grep -c ' rebase  0x00000000$')" = 2 ]
  not grep -q '_mh_\|dso_handle' <(nm -a $t/$f)
done

# The name is only for its kind.
cat <<EOF | $CC -o $t/main.o -c -xc -
int main() {}
EOF
not $CC --ld-path=$mold -o $t/exe $t/main.o $t/dylib.o 2> $t/log
grep -q __mh_dylib_header $t/log
