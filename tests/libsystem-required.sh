#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld64 takes a dynamic image that would load no dylib at all for one
# linked without libSystem by mistake, and refuses it: an executable
# unless -static, and a dylib or bundle (-dylib after -static still
# makes a dylib). Any dylib left after -dead_strip_dylibs will do,
# libSystem or not, and so will a bundle loader. libsystem_kernel,
# which libSystem is built on, and a link with an exit-asm.o are let
# off.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl _main
_main:
  ret
EOF
cat > $t/libfoo.tbd <<EOF
--- !tapi-tbd
tbd-version:     4
targets:         [ $ARCH-macos ]
install-name:    '/usr/local/lib/libfoo.dylib'
exports:
  - targets:         [ $ARCH-macos ]
    symbols:         [ _foo ]
...
EOF
sdk=$(xcrun --show-sdk-path)
link() { $mold -arch $ARCH -platform_version macos 14.0 14.0 -syslibroot "$sdk" "$@"; }
msg='dynamic executables or dylibs must link with libSystem.dylib'

not link -o $t/exe $t/a.o 2> $t/log1
grep -q "$msg" $t/log1
not link -dylib -o $t/b.dylib $t/a.o 2> $t/log2
grep -q "$msg" $t/log2
not link -bundle -o $t/c.bundle $t/a.o 2> $t/log3
grep -q "$msg" $t/log3
not link -static -dylib -o $t/d.dylib $t/a.o 2> $t/log4
grep -q "$msg" $t/log4
not link -dylib -undefined dynamic_lookup -o $t/e.dylib $t/a.o 2> $t/log5
grep -q "$msg" $t/log5
link -static -e _main -o $t/exe2 $t/a.o

link -dylib -o $t/f.dylib $t/a.o $t/libfoo.tbd
not link -dylib -o $t/g.dylib $t/a.o $t/libfoo.tbd -dead_strip_dylibs 2> $t/log6
grep -q "$msg" $t/log6
# libSystem stays whether or not anything uses it.
link -dylib -o $t/h.dylib $t/a.o -lSystem -dead_strip_dylibs

link -o $t/exe3 $t/a.o -lSystem
link -bundle -o $t/i.bundle $t/a.o -bundle_loader $t/exe3
not link -bundle -o $t/j.bundle $t/a.o -bundle_loader $t/exe3 -dead_strip_dylibs 2> $t/log7
grep -q "$msg" $t/log7

link -dylib -o $t/k.dylib $t/a.o -install_name /usr/lib/system/libsystem_kernel.dylib
cp $t/a.o $t/exit-asm.o
link -dylib -o $t/l.dylib $t/exit-asm.o

# Before macOS 12, a stub binds lazily, entering dyld through
# libSystem's dyld_stub_binder, but an image that loads no dylib is
# refused for that first.
cat <<EOF2 | $CC -o $t/m.o -c -xc - -mmacosx-version-min=11.0
void foo(void);
int main() { foo(); return 0; }
EOF2
not $mold -arch $ARCH -platform_version macos 11.0 11.0 -syslibroot "$sdk" -o $t/exe4 $t/m.o \
  -undefined dynamic_lookup 2> $t/log8
grep -q "$msg" $t/log8

# With a dylib that doesn't export dyld_stub_binder, an image that may
# look symbols up dynamically binds it so too, with a flat lookup.
link -o $t/exe5 $t/m.o $t/libfoo.tbd -no_fixup_chains -undefined dynamic_lookup
objdump --macho --bind $t/exe5 | grep -q 'flat-namespace *dyld_stub_binder'
link -o $t/exe6 $t/m.o $t/libfoo.tbd -no_fixup_chains -U dyld_stub_binder
objdump --macho --bind $t/exe6 | grep -q 'flat-namespace *dyld_stub_binder'
