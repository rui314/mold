#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime gives the libraries exports moved to ($ld$previous) their
# load commands after all the others, even the auto-linked ones: by the
# library they moved from, in load-command order, and then by the first
# export that moved. libzzz, all of whose exports moved, has no load
# command, but its moved export's library keeps libzzz's place.
tbd() {
  local inst=$1; shift
  local syms=$(printf "'%s', " "$@")
  cat <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-macos, arm64-macos ]
install-name:    '$inst'
current-version: 1
exports:
  - targets:         [ x86_64-macos, arm64-macos ]
    symbols:         [ ${syms%, } ]
...
EOF
}

tbd /mmm/libfoo.dylib _foo _bar _baz \
  '$ld$previous$/ddd/libold.dylib$$1$10.15$14.0$_bar$' \
  '$ld$previous$/ccc/libold2.dylib$$1$10.15$14.0$_foo$' > $t/libfoo.tbd
tbd /zzz/libzzz.dylib _zzz '$ld$previous$/bbb/libold3.dylib$$1$10.15$14.0$_zzz$' > $t/libzzz.tbd
tbd /bbb/libauto.dylib _auto _auto3 \
  '$ld$previous$/aaa/libold4.dylib$$1$10.15$14.0$_auto3$' > $t/libauto.tbd

cat <<'EOF' | $CC -mmacos-version-min=11.0 -o $t/a.o -c -xc -
__asm__(".linker_option \"-lauto\"");
void foo(void), bar(void), baz(void), zzz(void), aut(void) __asm__("_auto"), auto3(void);
int main() { foo(); bar(); baz(); zzz(); aut(); auto3(); }
EOF

$CC --ld-path=$mold -mmacos-version-min=11.0 -o $t/exe $t/a.o $t/libzzz.tbd $t/libfoo.tbd -L$t
otool -L $t/exe | tail -n +2 | awk '{print $1}' | tr '\n' ' ' > $t/libs
[ "$(cat $t/libs)" = "/mmm/libfoo.dylib /usr/lib/libSystem.B.dylib /bbb/libauto.dylib \
/bbb/libold3.dylib /ddd/libold.dylib /ccc/libold2.dylib /aaa/libold4.dylib " ]
