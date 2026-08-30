#!/usr/bin/env bash
. $(dirname $0)/common.inc

suffix=$(printf 'x%.0s' {1..300})
plain_name=plain_$suffix
versioned_name=versioned_$suffix
defsym_name=defsym_$suffix

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
void $plain_name() {}
void versioned_impl() {}
__asm__(".symver versioned_impl, $versioned_name@@VER");
EOF

echo 'VER {};' > $t/a.ver
$CC -B. -shared -o $t/b.so $t/a.o -Wl,--version-script=$t/a.ver

readelf --wide --dyn-syms $t/b.so > $t/log
grep -F "$plain_name" $t/log
grep -F "$versioned_name@@VER" $t/log

$CC -B. -shared -o $t/c.so $t/a.o -Wl,--version-script=$t/a.ver \
  -Wl,-defsym=$defsym_name=42
readelf --wide --symbols $t/c.so | grep -F "$defsym_name"
