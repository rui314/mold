#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime implies -init_offsets with chained fixups for an image dyld
# loads, but not for a -static one, whose loader runs __mod_init_func
# itself: XNU even renames the section into __DATA_CONST. It stays
# __DATA,__mod_init_func whatever the deployment target or
# -fixup_chains say; only -init_offsets converts it.
cat <<EOF | $CC -o $t/a.o -c -xassembler -
.text
.globl __start
.p2align 2
__start:
  ret
.section __DATA,__mod_init_func,mod_init_funcs
.p2align 3
.quad __start
EOF

sects() { otool -l $1 | awk '$1 == "sectname" { s = $2 } $1 == "segname" && s != "" { printf "%s,%s ", $2, s; s = "" }'; }

$mold -arch $ARCH -static -e __start -platform_version macos 26.0 26.0 $t/a.o -o $t/exe1
sects $t/exe1 | grep -q '__DATA,__mod_init_func'

$mold -arch $ARCH -static -e __start -fixup_chains $t/a.o -o $t/exe2
sects $t/exe2 | grep -q '__DATA,__mod_init_func'

$mold -arch $ARCH -static -e __start -platform_version macos 26.0 26.0 $t/a.o -o $t/exe3 \
  -rename_section __DATA __mod_init_func __DATA_CONST __mod_init_func
sects $t/exe3 | grep -q '__DATA_CONST,__mod_init_func'

$mold -arch $ARCH -static -e __start -init_offsets $t/a.o -o $t/exe4
sects $t/exe4 > $t/sects4
grep -q '__TEXT,__init_offsets' $t/sects4
not grep -q '__mod_init_func' $t/sects4
