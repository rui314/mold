#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -xc - -o $t/main.o
#include <dlfcn.h>
#include <stdio.h>
int main() {
  int (*fn)() = dlsym(RTLD_DEFAULT, "forced");
  printf("%d\n", fn ? fn() : -1);
}
EOF
echo _forced > $t/exports
for compile in native lto; do
  flags=
  if [ "$compile" = lto ]; then flags=-flto; fi
  echo 'int forced() { return 42; }' | $CC $flags -c -xc - -o $t/forced.o
  for retain in -u,_forced -exported_symbol,_forced -exported_symbols_list,$t/exports -export_dynamic; do
    $CC --ld-path=$mold $t/main.o $t/forced.o -Wl,-dead_strip,$retain -o $t/exe
    $t/exe | grep '^42$'
  done
  rm -f $t/libforced.a
  ar rcs $t/libforced.a $t/forced.o
  $CC --ld-path=$mold $t/main.o $t/libforced.a -Wl,-dead_strip,-u,_forced -o $t/archive
  $t/archive | grep '^42$'
done
