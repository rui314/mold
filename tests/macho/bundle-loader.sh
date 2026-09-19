#!/bin/bash
source "$(dirname "$0")"/common.inc

# -bundle_loader: a bundle's undefined symbols may resolve to the
# executable that will load it, bound at run time to the main
# executable (special ordinal -1, BIND_SPECIAL_DYLIB_MAIN_EXECUTABLE;
# 0 would be the bundle itself) with no LC_LOAD_DYLIB. Xcode links
# every app-hosted XCTest bundle this way, against the app linked
# with -export_dynamic.
cat <<EOF | $CC -o $t/host.o -c -xc -
#include <dlfcn.h>
#include <stdio.h>
int host_value = 41;
int host_func(void) { return host_value + 1; }
int main(int argc, char **argv) {
  void *h = dlopen(argv[1], RTLD_NOW);
  if (!h) { printf("dlopen: %s\n", dlerror()); return 1; }
  int (*plugin)(void) = dlsym(h, "plugin_func");
  printf("%d\n", plugin());
}
EOF
$CC --ld-path=$mold -o $t/host $t/host.o -Wl,-export_dynamic

cat <<EOF | $CC -o $t/plugin.o -c -xc -
extern int host_value;
int host_func(void);
int plugin_func(void) { return host_func() * 10 + host_value; }
EOF
$CC --ld-path=$mold -bundle -o $t/plugin.bundle $t/plugin.o -Wl,-bundle_loader,$t/host

# The host is not a load command, and the binds name the main
# executable.
otool -L $t/plugin.bundle > $t/libs
not grep -q host $t/libs
nm -m $t/plugin.bundle | grep -q 'undefined.*_host_func (from executable)'
dyld_info -fixups $t/plugin.bundle | grep -q '_host_func\|_host_value'

$t/host $t/plugin.bundle | grep -q '^461$'

# Xcode also passes -undefined dynamic_lookup, which makes the bundle
# use classic dyld info with lazy binding. The lazy binds must name
# the main executable too: with the ordinal for "this image" instead,
# dyld's lazy binder cannot find the symbol and the first call through
# the stub jumps to address zero (Hammerspoon's test bundle crashed in
# every test's setUp).
$CC --ld-path=$mold -bundle -o $t/plugin2.bundle $t/plugin.o -Wl,-bundle_loader,$t/host -Wl,-undefined,dynamic_lookup
dyld_info -fixups $t/plugin2.bundle > $t/fixups2
grep -q 'lazy-bind *<main-executable>/_host_func' $t/fixups2
grep -q 'bind *<main-executable>/_host_value' $t/fixups2
not grep -q 'this-image' $t/fixups2
$t/host $t/plugin2.bundle | grep -q '^461$'
