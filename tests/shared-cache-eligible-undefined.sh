#!/bin/bash
source "$(dirname "$0")"/common.inc

# The dyld shared cache builder binds each of a cached dylib's imports
# to the dylib that exports it, once and for all, so a dylib bound for
# the cache may not leave a symbol to a flat lookup at run time.
# ld-prime refuses -flat_namespace for one, and -undefined
# dynamic_lookup (or suppress) or -U, whether or not anything is left
# undefined; -not_for_dyld_shared_cache opts out, and other install
# names aren't checked.
echo 'int missing(void); int foo(void) { return missing(); }' | $CC -o $t/a.o -c -xc -
echo 'int bar(void) { return 1; }' | $CC -o $t/b.o -c -xc -

not $CC --ld-path=$mold -o $t/a.dylib -shared $t/a.o \
  -Wl,-install_name,/usr/lib/liba.dylib -Wl,-undefined,dynamic_lookup 2> $t/log1
grep -q "Shared cache eligible dylibs cannot use '-undefined dynamic_lookup' or '-U' to find symbols. Remove these options or opt out of the shared cache using the build setting 'LD_SHARED_CACHE_ELIGIBLE=NO' (or linker flag '-not_for_dyld_shared_cache')" $t/log1

not $CC --ld-path=$mold -o $t/b.dylib -shared $t/b.o \
  -Wl,-install_name,/System/Library/Frameworks/B.framework/B -Wl,-U,_missing 2> $t/log2
grep -q "cannot use '-undefined dynamic_lookup' or '-U'" $t/log2

not $CC --ld-path=$mold -o $t/b.dylib -shared $t/b.o \
  -Wl,-install_name,/usr/lib/libb.dylib -Wl,-undefined,suppress 2> $t/log3
grep -q "cannot use '-undefined dynamic_lookup' or '-U'" $t/log3

not $CC --ld-path=$mold -o $t/a.dylib -shared $t/a.o -Wl,-install_name,/usr/lib/liba.dylib \
  -Wl,-flat_namespace -Wl,-undefined,dynamic_lookup 2> $t/log4
grep -q "Shared cache eligible dylibs cannot use '-flat_namespace'.  Remove '-flat_namespace' or opt out of the shared cache using the build setting 'LD_SHARED_CACHE_ELIGIBLE=NO' (or linker flag '-not_for_dyld_shared_cache')" $t/log4

$CC --ld-path=$mold -o $t/c.dylib -shared $t/a.o -Wl,-install_name,/usr/lib/liba.dylib \
  -Wl,-undefined,dynamic_lookup -Wl,-not_for_dyld_shared_cache
$CC --ld-path=$mold -o $t/d.dylib -shared $t/a.o -Wl,-install_name,/usr/local/lib/liba.dylib \
  -Wl,-undefined,dynamic_lookup

# ld-prime refuses the flat namespace before it looks at the options
# only a main executable takes, but after those no dylib takes at all.
not $CC --ld-path=$mold -o $t/a.dylib -shared $t/a.o -Wl,-install_name,/usr/lib/liba.dylib \
  -Wl,-flat_namespace -Wl,-pagezero_size,0x1000 -Wl,-e,_foo 2> $t/log5
grep -q "Shared cache eligible dylibs cannot use '-flat_namespace'" $t/log5
not grep -q 'only be used when linking\|ignoring -e' $t/log5
not $CC --ld-path=$mold -o $t/a.dylib -shared $t/a.o -Wl,-install_name,/usr/lib/liba.dylib \
  -Wl,-flat_namespace -Wl,-client_name,foo 2> $t/log6
grep -q -- '-client_name can only be used' $t/log6

# ld-prime warns about a run path before it refuses -U, but refuses
# -flat_namespace before either.
not $CC --ld-path=$mold -o $t/e.dylib -shared $t/b.o -Wl,-install_name,/usr/lib/libe.dylib \
  -Wl,-U,_missing -Wl,-rpath,/foo 2> $t/log7
grep -A1 'OS dylibs should not add rpaths' $t/log7 | grep -q "cannot use '-undefined dynamic_lookup' or '-U'"
not $CC --ld-path=$mold -o $t/e.dylib -shared $t/b.o -Wl,-install_name,/usr/lib/libe.dylib \
  -Wl,-flat_namespace -Wl,-rpath,/foo 2> $t/log8
grep -q "cannot use '-flat_namespace'" $t/log8
not grep -q rpaths $t/log8
