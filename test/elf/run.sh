#!/bin/bash
export LC_ALL=C
set -e
CC="${TEST_CC:-cc}"
CXX="${TEST_CXX:-c++}"
GCC="${TEST_GCC:-gcc}"
GXX="${TEST_GXX:-g++}"
MACHINE="${MACHINE:-$(uname -m)}"
testname=$(basename "$0" .sh)
echo -n "Testing $testname ... "
t=out/test/elf/$MACHINE/$testname
mkdir -p $t

[ "$CC" = cc ] || { echo skipped; exit; }

# ASAN doesn't work with LD_PRELOAD
nm mold-wrapper.so | grep -q '__[at]san_init' && { echo skipped; exit; }

cat <<'EOF' | $CC -xc -c -o $t/a.o -
#include <stdio.h>

int main() {
  printf("Hello\n");
  return 0;
}
EOF

LD_PRELOAD=`pwd`/mold-wrapper.so MOLD_PATH=`pwd`/mold \
  $GCC -o $t/exe $t/a.o -B/usr/bin
readelf -p .comment $t/exe > $t/log
grep -q mold $t/log

./mold -run env | grep -q '^MOLD_PATH=.*/mold$'

./mold -run /usr/bin/ld --version | grep -q mold
./mold -run /usr/bin/ld.lld --version | grep -q mold
./mold -run /usr/bin/ld.gold --version | grep -q mold

rm -f $t/ld $t/ld.lld $t/ld.gold $t/foo.ld
touch $t/ld $t/ld.lld $t/ld.gold
echo "#!/bin/sh" >$t/foo.ld
chmod 755 $t/ld $t/ld.lld $t/ld.gold $t/foo.ld

./mold -run $t/ld --version | grep -q mold
./mold -run $t/ld.lld --version | grep -q mold
./mold -run $t/ld.gold --version | grep -q mold
./mold -run $t/foo.ld --version | grep -q mold && false

cat <<'EOF' > $t/sh
#!/bin/sh
$1 --version
EOF

chmod 755 $t/sh

./mold -run $t/sh ld --version | grep -q mold
./mold -run $t/sh foo.ld --version >& /dev/null | grep -q mold && false

./mold -run $t/sh $t/ld --version | grep -q mold
./mold -run $t/sh $t/ld.lld --version | grep -q mold
./mold -run $t/sh $t/ld.gold --version | grep -q mold
./mold -run $t/sh $t/foo.ld --version | grep -q mold && false

echo OK
