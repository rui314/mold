#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Response files and --retain-symbols-file are read before -C changes
# the directory, so their names are relative to the original one.
mkdir -p $t/dir
cat <<EOF | $CC -c -o $t/dir/a.o -xc -
void _start() {}
EOF

echo _start > $t/syms
echo "--retain-symbols-file $t/syms" > $t/rsp
rm -f $t/dir/exe.repro.tar

./mold -C $t/dir @$t/rsp a.o -o exe --repro
tar -tf $t/dir/exe.repro.tar > $t/log
grep '/rsp$' $t/log
grep '/syms$' $t/log
tar -xOf $t/dir/exe.repro.tar $(grep '/dir/a.o$' $t/log) | cmp - $t/dir/a.o
