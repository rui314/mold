#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A defined dynamic symbol with version index 0 (VER_NDX_LOCAL) is
# ill-formed. We used to silently ignore such symbols, which turned into
# confusing "undefined symbol" errors down the line. Make sure we report
# a proper error instead. https://github.com/rui314/mold/issues/1534

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
int foo() { return 3; }
int bar() { return 5; }
EOF

echo 'VER1 { global: bar; };' > $t/a.ver
$CC -B. -shared -o $t/a.so $t/a.o -Wl,--version-script=$t/a.ver

# Rewrite foo's .gnu.version entry with 0. No released linker emits index 0
# for a defined symbol, so patch the file with dd.
idx=$(readelf --dyn-syms $t/a.so | awk '$8 == "foo" { sub(/:/, "", $1); print $1 }')
off=$(readelf -SW $t/a.so | sed 's/\[ *[0-9]*\]//' |
      awk '$1 == ".gnu.version" { print $4 }')
[ -n "$idx" ] || skip
[ -n "$off" ] || skip

off=$((16#$off + idx * 2))
case $(od -A n -t x1 -j $off -N 2 $t/a.so | tr -d ' ') in
0100 | 0001) ;;
*) skip ;;
esac

printf '\0\0' | dd of=$t/a.so bs=1 seek=$off conv=notrunc status=none

cat <<EOF | $CC -c -o $t/b.o -xc -
int foo();
int main() { return foo() == 3 ? 0 : 1; }
EOF

! $CC -B. -o $t/exe $t/b.o $t/a.so -Wl,-rpath,$t 2> $t/log
grep 'invalid version index 0 for defined symbol foo' $t/log
