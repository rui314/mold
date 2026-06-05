#!/bin/bash
. $(dirname $0)/common.inc

# Regression test for issue #1595: --pack-dyn-relocs=android on aarch64 can
# cause set_osec_offsets() to enter an infinite layout oscillation loop
# between two sizes of .rela.dyn, hanging the linker indefinitely.
#
# Root cause (from the issue):
#   A .balign 16384 section forces B's address to be 16 KiB-aligned.
#   A's address is .rela.dyn-size bytes earlier. The relocation
#   addend B - A crosses a 2-byte / 3-byte SLEB128 boundary depending
#   on whether .rela.dyn is 7392 or 7393 bytes. The encoder's output
#   size in turn depends on the addend, so the loop ping-pongs between
#   7392 and 7393 forever.
#
# We use the exact reproducer assembly from the issue (Jwata). The
# unpatched linker hangs in encode_android(); the patched linker
# converges after capping the iteration count and applying a 1-byte
# pad to break the symmetry.

# Find a working aarch64-linux-android assembler. The .NET Android SDK
# ships one under Microsoft.Android.Sdk.Linux; distro packages may also
# provide it. We probe a few common locations before giving up.
find_android_as() {
  for cand in \
      /usr/bin/aarch64-linux-android-as \
      /usr/local/bin/aarch64-linux-android-as \
      $(find /usr/share/dotnet/packs/Microsoft.Android.Sdk.Linux -name aarch64-linux-android-as -print -quit 2>/dev/null); do
    [ -x "$cand" ] && echo "$cand" && return 0
  done
  return 1
}
AS=$(find_android_as) || skip

cat <<'EOF' > $t/repro.S
    .section .custom_rodata_a, "a", %progbits
    .hidden A
    .global A
A:
    .space 8

    .section .custom_rodata_b_huge, "a", %progbits
    .balign 16384
    .hidden B
    .global B
B:
    .space 8

    .section .data, "aw", %progbits
    .rept 7439
    .quad A
    .endr

    .global P_1
P_1:
    .quad A

    .global P_2
P_2:
    .quad B
EOF

"$AS" -o $t/repro.o $t/repro.S

# The unpatched linker hangs indefinitely (issue #1595). The patched
# linker converges in well under a second. 30s is 6 orders of magnitude
# more than the converged path needs, so a real hang is unmistakable.
timeout 30 mold -shared -o $t/repro.so $t/repro.o \
  --pack-dyn-relocs=android -z max-page-size=16384
[ $? -eq 0 ] || { echo "mold failed to converge in 30s (regression of #1595)" >&2; exit 1; }

# Sanity check: the output must be a valid aarch64 ELF shared object
# that actually used the ANDROID_RELA (APS2) encoding path.
readelf -h $t/repro.so | grep -qi aarch64 || { echo "wrong architecture" >&2; exit 1; }
readelf -SW $t/repro.so | grep -q ANDROID_RELA || { echo "ANDROID_RELA encoding not used" >&2; exit 1; }
