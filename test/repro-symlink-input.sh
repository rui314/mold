#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/a.o -xc -
int foo() { return 42; }
EOF

ln -sf a.o $t/alias.o
mkdir -p $t/real
cp $t/a.o $t/real/b.o
ln -sfn real $t/alias-dir

./mold -r --repro -o $t/relative.o $t/alias.o
./mold -r --repro -o $t/absolute.o "$PWD/$t/alias-dir/b.o"

ld=$PWD/mold
for name in relative absolute; do
  rm -rf $t/$name.o.repro
  tar -C $t -xf $t/$name.o.repro.tar
  if [ $name = relative ]; then
    test -f $t/$name.o.repro$PWD/$t/alias.o
  else
    test -f $t/$name.o.repro$PWD/$t/alias-dir/b.o
  fi
  (cd $t/$name.o.repro; "$ld" @response.txt)
  readelf -Ws "$t/$name.o.repro$PWD/$t/$name.o" | grep -E ' FUNC +GLOBAL +DEFAULT .* [0-9]+ +foo$'
done
