#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Input and output files are relative to the directory given by -C, but
# dynamic lists are read before mold changes the directory.
mkdir -p $t/dir
cat <<EOF | $CC -c -fPIC -o $t/dir/a.o -xc -
void _start() {}
void foo() {}
void bar() {}
void baz() {}
EOF

echo '{ foo; };' > $t/dyn1
echo '{ bar; };' > $t/dyn2

./mold -C $t/dir -pie -o exe a.o --dynamic-list=$t/dyn1 \
  --export-dynamic-symbol-list=$t/dyn2

readelf --dyn-syms $t/dir/exe > $t/log
grep -E ' foo( |$)' $t/log
grep -E ' bar( |$)' $t/log
not grep -E ' baz( |$)' $t/log
