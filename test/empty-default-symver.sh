#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A symbol exported with the default version but an empty (base) version
# name, i.e. `foo@@`, must be exported as a plain unversioned global even
# when a version script hides everything else with `local: *`. The explicit
# `@@` is an explicit export that takes precedence over the `local: *`
# wildcard, just as GNU ld does. Ceph's librados depends on this: its
# LIBRADOS_C_API_BASE_DEFAULT macro emits `.symver _fn, fn@@`.

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
void _foo() {}
__asm__(".symver _foo, foo@@");
EOF

echo '{ local: *; };' > $t/b.ver

$CC -B. -shared -o $t/c.so $t/a.o \
  -Wl,--version-script=$t/b.ver -Wl,--exclude-libs,ALL

readelf --dyn-syms $t/c.so > $t/log
grep -Eq ' FUNC +GLOBAL +DEFAULT +.*[0-9]+ foo$' $t/log
