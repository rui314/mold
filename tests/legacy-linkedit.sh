#!/bin/bash
source "$(dirname "$0")"/common.inc

# The macOS versions are the point of the test.
on_simulator && skip

# LC_DYLD_INFO came with macOS 10.6. ld-prime links an x86-64 image for
# an older macOS without it: dyld binds the GOT slots and lazy pointers
# by the indirect symbol table, binds pointers in data by external
# relocations (a slot keeping its addend, to which dyld adds the
# address) and slides an image that slides by local relocations. A lazy
# pointer starts out at its stub helper entry, which hands crt1.o's
# dyld_stub_binding_helper the pointer's address; there is no
# dyld_stub_binder, nor __dyld_private. An arm64 image has LC_DYLD_INFO
# whatever the target.

if [ $ARCH = arm64 ]; then
  echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
  $mold -arch arm64 -syslibroot $SDK -o $t/exe $t/a.o -lSystem \
    -platform_version macos 10.5 27.0 2> /dev/null
  otool -l $t/exe | grep -q 'cmd LC_DYLD_INFO_ONLY'
  exit
fi

cat > $t/a.c <<EOF
#include <stdio.h>
int f(void);
int main() { printf("Hello world\n"); printf("%d\n", f()); return 0; }
EOF

# Pointers to an import, to a weak definition (both bound), and to a
# local definition (slid).
cat > $t/b.c <<EOF
#include <stdio.h>
__attribute__((weak)) int weak_var = 3;
int local_var = 4;
int *p_weak = &weak_var;
int *p_local = &local_var;
int (*p_puts)(const char *) = puts;
int f(void) { p_puts("dylib"); return *p_weak + *p_local; }
EOF

$CC -c $t/a.c -o $t/a.o -mmacosx-version-min=10.5
$CC -c $t/b.c -o $t/b.o -mmacosx-version-min=10.5
$CC --ld-path=$mold -shared -o $t/libb.dylib $t/b.o -mmacosx-version-min=10.5 \
  -Wl,-install_name,@rpath/libb.dylib 2> /dev/null
$CC --ld-path=$mold -o $t/exe1 $t/a.o $t/libb.dylib -mmacosx-version-min=10.5 \
  -Wl,-rpath,@executable_path 2> /dev/null
$RUN $t/exe1 > $t/out1
grep -q 'Hello world' $t/out1
grep -q '^dylib$' $t/out1
grep -q '^7$' $t/out1

otool -l $t/exe1 > $t/lc1
not grep -q 'LC_DYLD_INFO' $t/lc1
otool -hv $t/exe1 | tail -1 > $t/flags1
not grep -q ' PIE' $t/flags1
nm -m $t/exe1 > $t/nm1
not grep -q 'dyld_stub_binder' $t/nm1
not grep -q '__dyld_private' $t/nm1

# Each entry: leaq lazy_ptr(%rip), %r11; jmp dyld_stub_binding_helper
helper=$(nm $t/exe1 | awk '$3 == "dyld_stub_binding_helper" { print $1 }' | sed 's/^0*//')
otool -v -s __TEXT __stub_helper $t/exe1 > $t/helper1
[ "$(grep -c 'leaq.*(%rip), %r11$' $t/helper1)" = 3 ]
[ "$(grep -c "jmp[[:space:]]*0x$helper$" $t/helper1)" = 3 ]
otool -I -v $t/exe1 > $t/ind1
grep -A4 '__la_symbol_ptr' $t/ind1 | grep -q ' _printf$'
otool -r -v $t/exe1 > $t/rel1
not grep -q 'Local relocation' $t/rel1

otool -l $t/libb.dylib > $t/lc2
not grep -q 'LC_DYLD_INFO' $t/lc2
otool -r -v $t/libb.dylib > $t/rel2
sed -n '/External relocation/,/Local relocation/p' $t/rel2 > $t/extrel2
grep -q ' _weak_var$' $t/extrel2
grep -q ' _puts$' $t/extrel2
sed -n '/Local relocation/,$p' $t/rel2 > $t/locrel2
[ "$(grep -c '(__DATA,__data)$' $t/locrel2)" = 1 ]

# A -pie executable slides by local relocations, its lazy pointers'
# among them.
$CC --ld-path=$mold -o $t/exe2 $t/a.o $t/libb.dylib -mmacosx-version-min=10.5 \
  -Wl,-pie,-rpath,@executable_path 2> /dev/null
$RUN $t/exe2 | grep -q 'Hello world'
otool -hv $t/exe2 | tail -1 | grep -q ' PIE'
otool -r -v $t/exe2 > $t/rel3
grep -q '(__TEXT,__stub_helper)$' $t/rel3

# From macOS 10.6 on, the image has LC_DYLD_INFO.
$CC --ld-path=$mold -o $t/exe3 $t/a.o $t/libb.dylib -mmacosx-version-min=10.6 \
  -Wl,-rpath,@executable_path 2> /dev/null
otool -l $t/exe3 | grep -q 'cmd LC_DYLD_INFO_ONLY'

# dyld_stub_binding_helper is a dead-strip root wherever imports bind
# lazily, whether a stub needs it or not.
echo 'void g(void) {}' | $CC -o $t/c.o -c -xc - -mmacosx-version-min=10.5
$CC --ld-path=$mold -shared -o $t/libc.dylib $t/c.o -mmacosx-version-min=10.5 \
  -Wl,-dead_strip 2> /dev/null
nm $t/libc.dylib | grep -q ' dyld_stub_binding_helper$'
$CC --ld-path=$mold -shared -o $t/libc2.dylib $t/c.o -mmacosx-version-min=10.5 \
  -Wl,-dead_strip,-bind_at_load 2> /dev/null
nm $t/libc2.dylib > $t/nm4
not grep -q 'dyld_stub_binding_helper' $t/nm4

# Without crt1.o (dylib1.o, bundle1.o), the helper entries have nothing
# to jump to.
cat > $t/d.s <<EOF
  .globl _h
_h:
  call _puts
  call _exit
  ret
EOF
$CC -c $t/d.s -o $t/d.o -mmacosx-version-min=10.5
not $mold -arch x86_64 -dylib -syslibroot $SDK -o $t/libd.dylib $t/d.o -lSystem \
  -platform_version macos 10.5 27.0 2> $t/log5
grep -q "target 'dyld_stub_binding_helper' does not have address" $t/log5

# The shared cache's builder reads a dylib's fixups from its opcodes or
# chains, never from legacy LINKEDIT, so ld-prime refuses one bound for
# the cache, by -add_split_seg_info or its install name.
not $CC --ld-path=$mold -shared -o $t/libe.dylib $t/b.o -mmacosx-version-min=10.5 \
  -Wl,-add_split_seg_info 2> $t/log6
grep -q 'Shared cache eligible dylibs must use bind opcodes or chained fixups' $t/log6
not $CC --ld-path=$mold -shared -o $t/libe.dylib $t/b.o -mmacosx-version-min=10.5 \
  -Wl,-install_name,/usr/lib/libe.dylib 2> $t/log7
grep -q 'Shared cache eligible dylibs must use bind opcodes or chained fixups' $t/log7
$CC --ld-path=$mold -shared -o $t/libe.dylib $t/b.o -mmacosx-version-min=10.5 \
  -Wl,-add_split_seg_info,-fixup_chains 2> /dev/null
