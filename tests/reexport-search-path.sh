#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime looks a re-exported library up by its install name's leaf,
# less the extension, in the library search path (-L, then the SDK's
# /usr/lib): /nonexistent/libB.1.dylib as libB.1.tbd or libB.1.dylib.
# For an absolute install name that lookup even comes before the install
# path. What the library found says about itself decides the rest: a
# public install name (/usr/lib/...) makes it a dylib of its own that
# symbols bind to, whatever name the re-exporter used.
mkdir -p $t/L $t/inst
stub() { # file install-name symbol [reexport]
  cat > $1 <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    '$2'
EOF
  if [ -n "$4" ]; then
    cat >> $1 <<EOF
reexported-libraries:
  - targets:         [ $ARCH-macos ]
    libraries:       [ '$4' ]
EOF
  fi
  cat >> $1 <<EOF
exports:
  - targets:         [ $ARCH-macos ]
    symbols:         [ $3 ]
...
EOF
}

from() { nm -m $1 | grep " _$2 " | sed 's/.*(from \(.*\))/\1/'; }

echo 'extern int sym1; int main() { return (long)&sym1 == 0; }' | $CC -o $t/a.o -c -xc -
stub $t/libA.tbd $t/libA.dylib _a /nonexistent/libB.1.dylib
stub $t/L/libB.1.tbd /nonexistent/libB.1.dylib _sym1
$CC --ld-path=$mold -o $t/exe1 $t/a.o $t/libA.tbd -L$t/L
[ "$(from $t/exe1 sym1)" = libA ]

echo 'extern int sym2; int main() { return (long)&sym2 == 0; }' | $CC -o $t/b.o -c -xc -
stub $t/libC.tbd $t/libC.dylib _c $t/inst/libD.dylib
stub $t/inst/libD.tbd $t/inst/libD.dylib _stale
stub $t/L/libD.tbd $t/inst/libD.dylib _sym2
$CC --ld-path=$mold -o $t/exe2 $t/b.o $t/libC.tbd -L$t/L
[ "$(from $t/exe2 sym2)" = libC ]

echo 'extern int sym3; int main() { return (long)&sym3 == 0; }' | $CC -o $t/c.o -c -xc -
stub $t/libE.tbd $t/libE.dylib _e /opt/nonexistent/libqq.dylib
stub $t/L/libqq.tbd /usr/lib/libqq.dylib _sym3
$CC --ld-path=$mold -o $t/exe3 $t/c.o $t/libE.tbd -L$t/L
[ "$(from $t/exe3 sym3)" = libqq ]
otool -L $t/exe3 | grep -q /usr/lib/libqq.dylib
