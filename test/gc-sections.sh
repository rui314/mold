#!/bin/bash
set -e
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF > $t/a.cc
#include <stdio.h>

int two() { return 2; }

int used_var1 = 1;
int used_var2 = two();
int unused_var1 = 3;
int unused_var2 = 4;

void used_fn1() {}
void used_fn2() { used_fn1(); }
void unused_fn1() {}
void unused_fn2() { unused_fn1(); }

int main() {
  printf("%d %d\n", used_var1, used_var2);
  used_fn2();
}
EOF

cflags="-ffunction-sections -fdata-sections -fuse-ld=`pwd`/../mold"

clang++ -o $t/exe1 $t/a.cc $cflags
readelf --symbols $t/exe1 > $t/log.1
cat $t/log.1 | grep -qv '0000000000000000 .* _Z10used_fn1v'
cat $t/log.1 | grep -qv '0000000000000000 .* _Z10used_fn2v'
cat $t/log.1 | grep -qv '0000000000000000 .* _Z10unused_fn1v'
cat $t/log.1 | grep -qv '0000000000000000 .* _Z10unused_fn2v'
cat $t/log.1 | grep -qv '0000000000000000 .* used_var1'
cat $t/log.1 | grep -qv '0000000000000000 .* used_var2'
cat $t/log.1 | grep -qv '0000000000000000 .* unused_var1'
cat $t/log.1 | grep -qv '0000000000000000 .* unused_var2'
$t/exe1 | grep -q '1 2'

clang++ -o $t/exe2 $t/a.cc $cflags -Wl,-gc-sections
readelf --symbols $t/exe2 > $t/log.2
cat $t/log.2 | grep -qv '0000000000000000 .* _Z10used_fn1v'
cat $t/log.2 | grep -qv '0000000000000000 .* _Z10used_fn2v'
cat $t/log.2 | grep -q  '0000000000000000 .* _Z10unused_fn1v'
cat $t/log.2 | grep -q  '0000000000000000 .* _Z10unused_fn2v'
cat $t/log.2 | grep -qv '0000000000000000 .* used_var1'
cat $t/log.2 | grep -qv '0000000000000000 .* used_var2'
cat $t/log.2 | grep -q  '0000000000000000 .* unused_var1'
cat $t/log.2 | grep -q  '0000000000000000 .* unused_var2'
$t/exe2 | grep -q '1 2'

echo OK
