#!/bin/bash
source "$(dirname "$0")"/common.inc

# A C++ inline function is linkonce_odr and unnamed_addr in bitcode, a
# definition no one can tell the copy of, which an image auto-hides as
# it does .weak_def_can_be_hidden. So a dylib doesn't export it, and
# ld-prime doesn't tell libLTO to preserve it: LTO inlines it into its
# one caller however big, as it would a static function, and no copy is
# left - with ThinLTO and merged modules alike.
cat <<EOF > $t/a.cpp
#include <cstdio>
inline int big_helper(int x) {
  int s = 0;
  for (int i = 0; i < x; i++) {
    s += i * x;
    if (s % 7 == 3) printf("a%d\n", s);
    if (s % 11 == 5) printf("b%d\n", s);
    if (s % 13 == 2) printf("c%d\n", s);
    if (s % 17 == 9) printf("d%d\n", s);
    if (s % 19 == 4) printf("e%d\n", s);
    if (s % 23 == 1) printf("f%d\n", s);
  }
  return s;
}
int exported_fn(int x) { return big_helper(x) + 1; }
EOF
for lto in thin full; do
  $CXX -O2 -flto=$lto -c $t/a.cpp -o $t/a-$lto.o
  $CXX --ld-path=$mold -shared -o $t/a-$lto.dylib $t/a-$lto.o
  nm $t/a-$lto.dylib > $t/nm-$lto
  grep -q ' T __Z11exported_fni$' $t/nm-$lto
  not grep -q big_helper $t/nm-$lto
done
