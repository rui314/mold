#!/bin/bash
source "$(dirname "$0")"/common.inc

# A tentative definition (a common symbol) beats a dylib's definition
# of the name unless -commons use_dylibs; -commons error refuses one, and
# -warn_commons warns of each.
cat <<EOF | $CC -o $t/a.o -c -xc -
int zc = 1;
int ac = 2;
EOF

cat <<EOF | $CC -o $t/b.o -c -xc - -fcommon
int zc;
int ac;
int main() { return zc; }
EOF

$CC --ld-path=$mold -o $t/liba.dylib -shared $t/a.o

$CC --ld-path=$mold -o $t/exe1 $t/b.o $t/liba.dylib -Wl,-warn_commons 2> $t/log
nm -m $t/exe1 | grep -q '(__DATA,__common) external _ac'
nm -m $t/exe1 | grep -q '(__DATA,__common) external _zc'
grep -v '^+' $t/log | sed -E 's/^(ld|mold): //; s|\(/[^)]*/|(|g' > $t/log2
cat > $t/expected <<EOF
warning: using common symbol '_ac' (b.o) and ignoring definition from dylib '_ac' (liba.dylib)
warning: using common symbol '_zc' (b.o) and ignoring definition from dylib '_zc' (liba.dylib)
EOF
diff $t/log2 $t/expected

LD_WARN_COMMONS=1 $CC --ld-path=$mold -o $t/exe2 $t/b.o $t/liba.dylib 2> $t/log
grep -q "warning: using common symbol '_zc'" $t/log

# The dylib's definitions, even of the unreferenced _ac.
$CC --ld-path=$mold -o $t/exe3 $t/b.o $t/liba.dylib -Wl,-commons,use_dylibs \
  -Wl,-warn_commons 2> $t/log
not grep -q warning $t/log
nm -m $t/exe3 | grep -q '(undefined) external _ac (from liba)'
nm -m $t/exe3 | grep -q '(undefined) external _zc (from liba)'

not $CC --ld-path=$mold -o $t/exe4 $t/b.o $t/liba.dylib -Wl,-commons,error 2> $t/log
grep -q "common symbol '_ac' (/.*/b.o) conflicts with definition from dylib '_ac' (/.*/liba.dylib)" $t/log
not grep -q _zc $t/log

for arg in foo ''; do
  not $mold -o $t/exe5 $t/b.o -commons "$arg" 2> $t/log
  grep -q 'invalid option to -commons \[ ignore_dylibs | error \]' $t/log
done
not $mold -o $t/exe5 $t/b.o -commons 2> $t/log
grep -q 'invalid option to -commons \[ ignore_dylibs | error \]' $t/log
