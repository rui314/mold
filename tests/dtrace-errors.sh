#!/bin/bash
source "$(dirname "$0")"/common.inc
source "$(dirname "$0")"/dtrace.inc

# ld-prime fails the link when a provider's symbols make no DOF, with
# the messages of the libdtrace it has make it, then its own; and when
# code calls one of a provider's symbols that are no probe.
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

# fails NAME LINES...: links a main of the given lines, which must fail,
# and leaves its messages in $t/NAME.err, without the linker's prefix.
fails() {
  main "$@"
  local name=$1
  not $CC --ld-path=$mold -o $t/exe $t/$name.o 2> $t/$name.log
  sed -E -e '/^\+/d' -e '/linker command failed/d' -e 's/^(ld: |mold: error: )//' \
    $t/$name.log > $t/$name.err
}

# A probe no site fires, only an is-enabled test.
fails enabled "$call "'___dtrace_isenabled$p$x$v1' ".reference $stab" ".reference $typedefs"
cat > $t/expected <<EOF
error: probe x doesn't exist
error: Could not register probes
error creating dtrace DOF section
EOF
diff $t/enabled.err $t/expected

# No stability, or two.
fails nostab "$call "'___dtrace_probe$p$x$v1' ".reference $typedefs"
cat > $t/expected <<EOF
error: Must have a valid dtrace stability entry
error creating dtrace DOF section
EOF
diff $t/nostab.err $t/expected

fails twostab "$call "'___dtrace_probe$p$x$v1' ".reference $stab" ".reference $typedefs" \
  '.reference ___dtrace_stability$p$v1$5_5_4_1_1_0_1_1_0_5_6_5_7_3_2'
cat > $t/expected <<EOF
error: Found conflicting dtrace stability info:
___dtrace_stability\$p\$v1\$1_1_0_1_1_0_1_1_0_1_1_0_1_1_0
___dtrace_stability\$p\$v1\$5_5_4_1_1_0_1_1_0_5_6_5_7_3_2
error creating dtrace DOF section
EOF
diff $t/twostab.err $t/expected

# A type the D script doesn't know: "foo_t".
fails type "$call "'___dtrace_probe$p$x$v1$666f6f5f74' ".reference $stab" ".reference $typedefs"
{
  printf 'error: Could not compile reconstructed dtrace script:\n\n\n'
  printf 'provider p {\n\tprobe x(foo_t);\n};\n\n'
  for what in provider module function name args; do
    echo "#pragma D attributes PRIVATE/PRIVATE/UNKNOWN provider p $what"
  done
  printf '\n\nerror creating dtrace DOF section\n'
} > $t/expected
diff $t/type.err $t/expected

# A call of the stability symbol.
fails call "$call $stab" ".reference $typedefs"
echo 'Unexpected call to dtrace provider undef' > $t/expected
diff $t/call.err $t/expected

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
