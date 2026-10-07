#!/bin/bash
source "$(dirname "$0")"/common.inc

# The libraries exports moved to ($ld$previous) get load commands of
# their own, and the moved exports bind to them. libzzz, all of whose
# exports moved, has no load command. The command-line libraries keep
# their order. (ld-prime lists the moved-to libraries after all the
# others, by the library they moved from and then by the first export
# that moved.)
tbd() {
  local inst=$1; shift
  local syms=$(printf "'%s', " "$@")
  cat <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ x86_64-$PLATFORM, arm64-$PLATFORM ]
install-name:    '$inst'
current-version: 1
exports:
  - targets:         [ x86_64-$PLATFORM, arm64-$PLATFORM ]
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
otool -L $t/exe | tail -n +2 | awk '{print $1}' > $t/libs
[ "$(sort $t/libs | tr '\n' ' ')" = "/aaa/libold4.dylib /bbb/libauto.dylib /bbb/libold3.dylib \
/ccc/libold2.dylib /ddd/libold.dylib /mmm/libfoo.dylib /usr/lib/libSystem.B.dylib " ]
[ "$(grep -e libfoo -e libSystem $t/libs | tr '\n' ' ')" = "/mmm/libfoo.dylib /usr/lib/libSystem.B.dylib " ]
dyld_info -fixups $t/exe | grep bind | awk '{print $NF}' | sort -u > $t/binds
grep -qx 'libfoo/_baz' $t/binds
grep -qx 'libold/_bar' $t/binds
grep -qx 'libold2/_foo' $t/binds
grep -qx 'libold3/_zzz' $t/binds
grep -qx 'libauto/_auto' $t/binds
grep -qx 'libold4/_auto3' $t/binds
