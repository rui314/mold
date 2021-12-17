#!/bin/bash
export LANG=
set -e
cd $(dirname $0)
mold=`pwd`/../../mold
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/../../out/test/elf/$(basename -s .sh $0)
mkdir -p $t

cat <<EOF > $t/a.cc
#include <stdio.h>

int two() { return 2; }

int live_var1 = 1;
int live_var2 = two();
int dead_var1 = 3;
int dead_var2 = 4;

void live_fn1() {}
void live_fn2() { live_fn1(); }
void dead_fn1() {}
void dead_fn2() { dead_fn1(); }

int main() {
  printf("%d %d\n", live_var1, live_var2);
  live_fn2();
}
EOF

cflags="-ffunction-sections -fdata-sections -fuse-ld=$mold"

clang++ -o $t/exe1 $t/a.cc $cflags
readelf --symbols $t/exe1 > $t/log.1
grep -qv live_fn1 $t/log.1
grep -qv live_fn2 $t/log.1
grep -qv dead_fn1 $t/log.1
grep -qv dead_fn2 $t/log.1
grep -qv live_var1 $t/log.1
grep -qv live_var2 $t/log.1
grep -qv dead_var1 $t/log.1
grep -qv dead_var2 $t/log.1
$t/exe1 | grep -q '1 2'

clang++ -o $t/exe2 $t/a.cc $cflags -Wl,-gc-sections
readelf --symbols $t/exe2 > $t/log.2
grep -q  live_fn1 $t/log.2
grep -q  live_fn2 $t/log.2
grep -qv dead_fn1 $t/log.2
grep -qv dead_fn2 $t/log.2
grep -q  live_var1 $t/log.2
grep -q  live_var2 $t/log.2
grep -qv dead_var1 $t/log.2
grep -qv dead_var2 $t/log.2
$t/exe2 | grep -q '1 2'

echo OK
