#!/bin/bash
set -e
cd $(dirname $0)
echo -n "Testing $(basename -s .sh $0) ... "
t=$(pwd)/tmp/$(basename -s .sh $0)
mkdir -p $t

echo 'ver_x { global: *; };' > $t/a.ver

cat <<EOF > $t/b.s
.globl foo, bar, baz
foo:
  nop
bar:
  nop
baz:
  nop
EOF

clang -fuse-ld=`pwd`/../mold -shared -o $t/c.so -Wl,-version-script,$t/a.ver $t/b.s
readelf --version-info $t/c.so > $t/log

fgrep -q 'Rev: 1  Flags: none  Index: 2  Cnt: 1  Name: ver_x' $t/log

echo OK
