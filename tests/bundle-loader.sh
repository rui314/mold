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
nm -m $t/plugin.bundle | grep 'undefined.*_host_func (from executable)'
dyld_info -fixups $t/plugin.bundle | grep '_host_func\|_host_value'

$t/host $t/plugin.bundle | grep '^461$'

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
$t/host $t/plugin2.bundle | grep '^461$'

# ld-prime warns about a loader nothing binds to only under
# -warn_unused_dylibs (a bundle is never bound for the shared cache),
# naming its real path, in its command-line place among the unused
# dylibs; -t lists it as given.
echo 'int unused_func(void) { return 1; }' | $CC -o $t/unused.o -c -xc -
echo 'int z(void) { return 1; }' | $CC -o $t/z.o -c -xc -
$CC --ld-path=$mold -o $t/libz1.dylib -shared $t/z.o -Wl,-install_name,@rpath/libz1.dylib
mkdir -p $t/sub
ln -sf ../host $t/sub/hostlink
$CC --ld-path=$mold -bundle -o $t/u1.bundle $t/unused.o -Wl,-bundle_loader,$t/host 2> $t/log1
not grep -q 'bundle loader' $t/log1
$CC --ld-path=$mold -bundle -o $t/u2.bundle $t/unused.o $t/libz1.dylib \
  -Wl,-bundle_loader,$t/sub/hostlink -Wl,-warn_unused_dylibs 2> $t/log2
grep -n 'but not using any symbols' $t/log2 | cut -d: -f1 | tr '\n' ' ' > $t/lines
[ "$(cat $t/lines)" = '1 2 ' ]
sed -n 2p $t/log2 | grep -q "linking with bundle loader (/.*/host) but not using any symbols from it"
$CC --ld-path=$mold -bundle -o $t/u3.bundle $t/unused.o -Wl,-bundle_loader,$t/sub/hostlink \
  -Wl,-t > $t/trace
grep -q 'sub/hostlink$' $t/trace
