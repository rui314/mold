#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A bundle referencing a symbol only its host defines, resolved at
# run time by flat lookup.
cat <<EOF2 | $CC -o $t/a.o -c -xc -
int host_value();
int plugin_value() { return host_value() + 1; }
EOF2

$CC --ld-path=$mold -bundle -o $t/plugin.bundle $t/a.o \
  -Wl,-undefined,dynamic_lookup

cat <<EOF2 | $CC -o $t/b.o -c -xc -
#include <dlfcn.h>
#include <stdio.h>
int host_value() { return 41; }
int main(int argc, char **argv) {
  void *h = dlopen(argv[1], RTLD_NOW);
  if (!h) { printf("dlopen: %s\n", dlerror()); return 1; }
  int (*fn)(void) = dlsym(h, "plugin_value");
  printf("%d\n", fn());
}
EOF2

$CC --ld-path=$mold -o $t/exe $t/b.o
$t/exe $t/plugin.bundle | grep '^42$'
