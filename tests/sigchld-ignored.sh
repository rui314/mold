#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int foo();
int main() { return foo(); }
EOF

# An ignored SIGCHLD is inherited across exec. The forked child is then
# reaped automatically, but mold must still report that the link failed.
# (On macOS the child is reaped so only if it exits before the parent
# waits for it, which the parent seldom fails to do first.)
(trap '' CHLD; not $mold -arch $ARCH -platform_version ${PLATFORM_VERSION:-macos 15.0 15.0} \
  -syslibroot $SDK -lSystem -o $t/exe $t/a.o)
