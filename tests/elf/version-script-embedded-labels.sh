#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -fPIC -c -o $t/a.o -xc -
int foo() { return 42; }
int bar() { return 43; }
EOF

# Colon-containing input names must not be split as visibility labels.
cp $t/a.o $t/local:input.o
echo 'VERSION { { local:*; }; }' > $t/local.script
./mold -shared -o $t/local.so $t/local:input.o -T $t/local.script
readelf -W --dyn-syms $t/local.so > $t/local.log
not grep -E ' (foo|bar)$' $t/local.log

cat <<EOF > $t/global.script
INPUT(local:input.o)
VERSION { V1 { global:foo; local:*; }; }
EOF
./mold -shared --no-undefined-version -o $t/global.so -T $t/global.script
readelf -W --dyn-syms $t/global.so > $t/global.log
grep -F 'foo@@V1' $t/global.log
not grep -E ' bar(@|$)' $t/global.log
