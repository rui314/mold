#!/bin/bash
. $(dirname $0)/common.inc

[ -z "$TRIPLE" ] || skip

build=$t/build

if cmake -S $(dirname $0)/.. -B $build -DBUILD_TESTING=OFF \
  -DCMAKE_CXX_COMPILER="$GXX" -DMOLD_USE_LIBCXX=ON >$t/configure.log 2>&1; then
  false
fi

grep -F 'MOLD_USE_LIBCXX requires Clang' $t/configure.log
