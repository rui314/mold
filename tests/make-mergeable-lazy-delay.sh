#!/bin/bash
source "$(dirname "$0")"/common.inc

# A mergeable dylib may name a lazy-load or delay-init dylib it uses
# nothing of: ld-prime links it as any other, the delay-init dylib's
# load command and _dlopen kept, and the empty subsection that keeps
# __dyld_lazy_load alive and its imports recorded, so that an image
# that merges it, linked by either linker, gets them too.
cat <<EOF | $CC -o $t/bar.o -c -xc -
int bv = 10;
int bar(void) { return 4; }
EOF
cat <<EOF | $CC -o $t/foo.o -c -xc -
extern int bv;
int bar(void);
int foo(void) { return bar() + bv; }
EOF
cat <<EOF | $CC -o $t/baz.o -c -xc -
int baz(void) { return 1; }
EOF
cat <<EOF | $CC -o $t/main.o -c -xc -
int baz(void);
int main() { return baz() - 1; }
EOF

mkdir -p $t/lib $t/Frameworks/Bar.framework $t/m1 $t/m2
$CC --ld-path=$mold -shared -o $t/lib/libbar.dylib $t/bar.o \
  -Wl,-install_name,@rpath/libbar.dylib
$CC --ld-path=$mold -shared -o $t/Frameworks/Bar.framework/Bar $t/bar.o \
  -Wl,-install_name,@rpath/Bar.framework/Bar

$CC --ld-path=$mold -shared -o $t/m1/libbaz.dylib $t/baz.o -L$t/lib -Wl,-make_mergeable \
  -Wl,-lazy-lbar -mmacosx-version-min=27.0 -Wl,-install_name,@rpath/libbaz.dylib
$CC --ld-path=$mold -shared -o $t/m2/libbaz.dylib $t/baz.o -L$t/lib -Wl,-make_mergeable \
  -Wl,-delay-lbar -mmacosx-version-min=27.0 -Wl,-install_name,@rpath/libbaz.dylib
for ldflag in --ld-path=$mold ""; do
  $CC $ldflag -o $t/exe1 $t/main.o -L$t/m1 -L$t/lib -Wl,-merge-lbaz \
    -mmacosx-version-min=27.0
  nm -m $t/exe1 > $t/syms1
  grep -q '(undefined) external __dyld_lazy_load (from libSystem)' $t/syms1
  $CC $ldflag -o $t/exe2 $t/main.o -L$t/m2 -L$t/lib -Wl,-merge-lbaz \
    -mmacosx-version-min=27.0
  nm -m $t/exe2 > $t/syms2
  grep -q '(undefined) external _dlopen (from libSystem)' $t/syms2
  otool -L $t/exe2 | grep -q libbar.dylib
done

# Code that reaches a symbol of a lazy-load or delay-init dylib is
# rewritten to go through a helper, which a mergeable dylib would keep
# under the compiler's fixup, so no link could merge it. mold refuses
# such a use; ld-prime crashes on -lazy-l and makes an unmergeable
# dylib of -delay-l, so the rest of this test is mold's own.
$mold -v 2>&1 | grep -q mold-macho || exit 0

for opt in -delay-lbar -delay_library,$t/lib/libbar.dylib -delay_framework,Bar; do
  not $CC --ld-path=$mold -shared -o $t/foo.dylib $t/foo.o -L$t/lib -F$t/Frameworks \
    -Wl,-make_mergeable -Wl,$opt 2> $t/log
  grep -q -- '-delay-l/-delay_library/-delay_framework cannot be used with -make_mergeable' $t/log
done

for opt in -lazy-lbar -lazy_library,$t/lib/libbar.dylib -lazy_framework,Bar; do
  not $CC --ld-path=$mold -shared -o $t/foo.dylib $t/foo.o -L$t/lib -F$t/Frameworks \
    -Wl,-make_mergeable -Wl,$opt -mmacosx-version-min=27.0 2> $t/log
  grep -q -- '-lazy-l/-lazy_library/-lazy_framework cannot be used with -make_mergeable' $t/log
done

# Before macOS 27, a lazy-load dylib links as any other.
$CC --ld-path=$mold -shared -o $t/foo.dylib $t/foo.o -L$t/lib -Wl,-make_mergeable \
  -Wl,-lazy-lbar -mmacosx-version-min=26.0 2> $t/log
grep -q "lazy-load will be ignored for 'bar'" $t/log
