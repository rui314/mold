#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime looks a symbol up in the libraries the command line names,
# in input order, and only then - after every archive - in the public
# libraries they re-export: nearest first (a private library in between
# counts as a step), and among the equally near by the install name of
# the library that re-exports them, then by their own. -framework Carbon
# -framework Foundation binds NSURLSession to Foundation, though Carbon
# reaches CFNetwork, which exports it too, through CoreServices.
doc() { # install-name reexports... -- exports...
  local name=$1; shift
  local re=()
  while [ "$1" != -- ]; do re+=("'$1'"); shift; done
  shift
  echo "--- !tapi-tbd"
  echo "tbd-version:     4"
  echo "targets:         [ $ARCH-macos ]"
  echo "install-name:    '$name'"
  if [ ${#re[@]} -gt 0 ]; then
    echo "reexported-libraries:"
    echo "  - targets:         [ $ARCH-macos ]"
    echo "    libraries:       [ $(IFS=,; echo "${re[*]}") ]"
  fi
  echo "exports:"
  echo "  - targets:         [ $ARCH-macos ]"
  echo "    symbols:         [ $(IFS=,; echo "$*") ]"
}

# libp re-exports libq, which defines sym; libr defines it itself.
rm -f $t/libq.tbd
{ doc /usr/lib/libp.dylib /usr/lib/libq.dylib -- _p; doc /usr/lib/libq.dylib -- _sym; echo ...; } > $t/libp.tbd
{ doc /usr/lib/libr.dylib -- _sym; echo ...; } > $t/libr.tbd
# liba reaches liba2 two steps away; libb reaches libb1 one step away.
{ doc /usr/lib/liba.dylib /usr/lib/liba1.dylib -- _a; doc /usr/lib/liba1.dylib /usr/lib/liba2.dylib -- _a1;
  doc /usr/lib/liba2.dylib -- _sym; echo ...; } > $t/liba.tbd
{ doc /usr/lib/libb.dylib /usr/lib/libb1.dylib -- _b; doc /usr/lib/libb1.dylib -- _sym; echo ...; } > $t/libb.tbd
# libm and libn re-export libzz and libaa: libm's comes first.
{ doc /usr/lib/libm.dylib /usr/lib/libzz.dylib -- _m; doc /usr/lib/libzz.dylib -- _sym; echo ...; } > $t/libm.tbd
{ doc /usr/lib/libn.dylib /usr/lib/libaa.dylib -- _n; doc /usr/lib/libaa.dylib -- _sym; echo ...; } > $t/libn.tbd
# libh re-exports libhz and libha: libha comes first.
{ doc /usr/lib/libh.dylib /usr/lib/libhz.dylib /usr/lib/libha.dylib -- _h; doc /usr/lib/libhz.dylib -- _sym;
  doc /usr/lib/libha.dylib -- _sym; echo ...; } > $t/libh.tbd
# libc reaches libpq through a private library, two steps away.
{ doc /usr/lib/libc.dylib $t/priv/libcp.dylib -- _c; doc $t/priv/libcp.dylib /usr/lib/libpq.dylib -- _cp;
  doc /usr/lib/libpq.dylib -- _sym; echo ...; } > $t/libc.tbd
echo 'int sym = 1;' | $CC -o $t/x.o -c -xc -
rm -f $t/libx.a
ar rcs $t/libx.a $t/x.o

echo 'extern int sym; int main() { return (long)&sym == 0; }' | $CC -o $t/a.o -c -xc -

from() {
  $CC --ld-path=$mold -o $t/exe $t/a.o -L$t "$@"
  nm -m $t/exe | grep ' _sym' | sed 's/.*(from \(.*\))/\1/; s/.* _sym$/defined/'
}

[ "$(from -lp -lr)" = libr ]
[ "$(from -lr -lp)" = libr ]
[ "$(from -la -lb)" = libb1 ]
[ "$(from -lb -la)" = libb1 ]
[ "$(from -lm -ln)" = libzz ]
[ "$(from -ln -lm)" = libzz ]
[ "$(from -lh)" = libha ]
[ "$(from -lc -lb)" = libb1 ]
[ "$(from -lp -lx)" = defined ]
[ "$(from -lp $t/libx.a)" = defined ]

# A library named after it was loaded as a re-export takes its naming's
# place.
{ doc /usr/lib/libq.dylib -- _sym; echo ...; } > $t/libq.tbd
[ "$(from -lp -lr -lq)" = libr ]
[ "$(from -lp -lq -lr)" = libq ]
