#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Skip if llvm-readelf isn't available; we use it to decode APS2.
command -v llvm-readelf >/dev/null || skip

cat <<EOF | $CC -o $t/a.o -fPIC -c -xc -
extern int x, y, z;
void *ptrs[] = { &x, &y, &z, &x, &y, &z };
int x = 1, y = 2, z = 3;
int main() { return 0; }
EOF

# Probe whether the host toolchain can produce a working PIE for this $CC.
$CC -o $t/probe $t/a.o -pie 2> /dev/null || skip

# --pack-dyn-relocs=android: all dynrels packed into APS2 .rela.dyn.
$CC -B. -o $t/exe1 $t/a.o -pie -Wl,--pack-dyn-relocs=android
llvm-readelf -SW $t/exe1 | grep -Ew '\.rela?\.dyn' | grep -E 'ANDROID_RELA?'
llvm-readelf --dynamic $t/exe1 | grep -E 'ANDROID_RELA?\b'
llvm-readelf --dynamic $t/exe1 | grep -E 'ANDROID_RELA?SZ'
# llvm-readelf decodes APS2 back to individual relocations. Check the
# section was decoded with a non-zero count (we don't grep for a specific
# type name because llvm-readelf prints "Unknown" for relocation types
# of arches it doesn't recognize, e.g. SH4/M68K).
llvm-readelf -r $t/exe1 | grep -E "'\.rela?\.dyn'.*contains [1-9]"

# --pack-dyn-relocs=android+relr: R_RELATIVE -> .relr.dyn, others -> APS2.
$CC -B. -o $t/exe2 $t/a.o -pie -Wl,--pack-dyn-relocs=android+relr
llvm-readelf -SW $t/exe2 | grep -Ew '\.rela?\.dyn' | grep -E 'ANDROID_RELA?'
llvm-readelf -SW $t/exe2 | grep -F .relr.dyn
llvm-readelf --dynamic $t/exe2 | grep -E 'ANDROID_RELA?\b'
llvm-readelf --dynamic $t/exe2 | grep -Ew 'RELR'
