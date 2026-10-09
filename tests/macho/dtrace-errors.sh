#!/bin/bash
source "$(dirname "$0")"/common.inc

# A probe symbol of another encoding than the v1 of dtrace -h fails the
# link, as its fields can't be read.
if [ $ARCH = arm64 ]; then call=bl; else call=call; fi
stab='___dtrace_stability$p$v1$1_1_0_1_1_0_1_1_0_1_1_0_1_1_0'
typedefs='___dtrace_typedefs$p$v2'

# main NAME LINES...: assembles a main of the given lines into $t/NAME.o.
main() {
  local name=$1
  shift
  { echo '.globl _main'; echo '_main:'; printf '  %s\n' "$@"; echo '  ret'
    echo '.subsections_via_symbols'; } > $t/$name.s
  $CC -o $t/$name.o -c $t/$name.s
}

main v2 "$call "'___dtrace_probe$p$x$v2$696e74' ".reference $stab" ".reference $typedefs"
not $CC --ld-path=$mold -o $t/exe $t/v2.o 2> $t/log

# -no_dtrace_dof makes no DOF and leaves the sites calls of address 0.
main nodof "$call "'___dtrace_probe$p$x$v1' ".reference $stab" ".reference $typedefs"
$CC --ld-path=$mold -o $t/exe $t/nodof.o
not $CC --ld-path=$mold -o $t/exe $t/nodof.o -Wl,-no_dtrace_dof 2> $t/log
if [ $ARCH = arm64 ]; then
  grep -qF "$t/nodof.o: _main+0x0: B/BL out of range" $t/log
else
  grep -qF "$t/nodof.o: _main+0x1: 32-bit RIP-relative" $t/log
fi
grep -qF "to 0x00000000 ('___dtrace_probe\$p\$x\$v1')" $t/log

# The rest is mold's own: ld-prime has libdtrace rebuild the provider's
# D script and compile it, which refuses a probe that only is-enabled
# tests name, and a provider without its stability and typedefs
# symbols, though dtrace -h writes neither. mold makes a DOF of them:
# the probe has no arguments, the provider D's default attributes.
$mold -v 2>&1 | grep -q mold-macho || exit 0
main enabled "$call "'___dtrace_isenabled$p$x$v1'
$CC --ld-path=$mold -o $t/exe $t/enabled.o
dof_dump $t/exe > $t/dof
cat > $t/expected <<EOF
dof __dof_p p flags 0xf align 0
attrs 0x01010000 0x01010000 0x01010000 0x01010000 0x01010000
probe x() in main: 0 sites, 1 tests
EOF
diff $t/dof $t/expected
