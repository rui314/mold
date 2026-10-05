#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -xc -o $t/a.o -
#include <stdio.h>
int main() { printf("Hello\n"); }
EOF

mkdir -p $t/tmp

# The intermediate file is written to the directory given by --temp-dir and
# moved to the final location once linking is complete.
$CC -B. -o $t/exe $t/a.o -Wl,--temp-dir=$t/tmp
$QEMU $t/exe | grep -q Hello
test -z "$(ls -A $t/tmp)"

# The command-line option takes precedence over the environment variable.
MOLD_TEMP_DIR=$t/nonexistent $CC -B. -o $t/exe $t/a.o -Wl,--temp-dir=$t/tmp
$QEMU $t/exe | grep -q Hello
test -z "$(ls -A $t/tmp)"

# The environment variable is another way to set the directory.
MOLD_TEMP_DIR=$t/tmp $CC -B. -o $t/exe $t/a.o
$QEMU $t/exe | grep -q Hello
test -z "$(ls -A $t/tmp)"

# A missing directory is reported as an error.
not $CC -B. -o $t/exe $t/a.o -Wl,--temp-dir=$t/nonexistent

# If another filesystem is available, verify that the intermediate file is
# copied there and then moved back across the filesystem boundary.
if command -v stat > /dev/null && stat -c %d /dev/shm > /dev/null 2>&1 &&
  [ "$(stat -c %d /dev/shm)" != "$(stat -c %d $t/tmp)" ]; then
  mkdir -p /dev/shm/mold-test-$$
  $CC -B. -o $t/exe $t/a.o -Wl,--temp-dir,/dev/shm/mold-test-$$
  $QEMU $t/exe | grep -q Hello
  test -z "$(ls -A /dev/shm/mold-test-$$)"

  # A failure during the copy removes both the intermediate file and the
  # partially written staging file. The failure is simulated by removing
  # write permission from the output directory, which makes creating the
  # staging file fail. Skip the check if we are running as root, as root
  # can create a file in a directory without write access.
  if [ "$(id -u)" != 0 ]; then
    mkdir -p $t/ro
    chmod a-w $t/ro
    not $CC -B. -o $t/ro/exe $t/a.o -Wl,--temp-dir,/dev/shm/mold-test-$$
    test -z "$(ls -A /dev/shm/mold-test-$$)"
    test -z "$(ls -A $t/tmp)"
    test -z "$(ls -A $t/ro)"
    chmod u+w $t/ro
  fi

  rm -rf /dev/shm/mold-test-$$
fi
