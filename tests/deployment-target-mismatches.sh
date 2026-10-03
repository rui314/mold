#!/bin/bash
source "$(dirname "$0")"/common.inc

# An object built for a newer OS version than the link's draws a
# warning, which -deployment_target_mismatches error makes the error
# that stops the link, and suppress silences.
cat <<EOF | $CC -o $t/a.o -c -xc - -mmacosx-version-min=13.0
int main() { return 0; }
EOF
cp $t/a.o $t/b.o

link() {
  $mold -arch $ARCH -syslibroot "$(xcrun --show-sdk-path)" -lSystem \
    -platform_version macos 12.0 12.0 -o $t/exe $t/a.o "$@"
}

link 2> $t/log
grep -q "warning: object file (.*/a.o) was built for newer 'macOS' version (13.0) than being linked (12.0)" $t/log
link -deployment_target_mismatches warn 2> $t/log
grep -q 'warning: object file' $t/log
link -deployment_target_mismatches suppress 2> $t/log
not grep -q 'object file' $t/log

not link $t/b.o -deployment_target_mismatches error 2> $t/log
grep -q "object file (.*/a.o) was built for newer 'macOS' version (13.0) than being linked (12.0)" $t/log
not grep -q 'warning' $t/log
grep -v "^+" $t/log > $t/msgs
not grep -q b.o $t/msgs

not link -deployment_target_mismatches foo 2> $t/log
grep -q -- '-deployment_target_mismatches invalid option (warning | error | suppress)' $t/log
not link -deployment_target_mismatches 2> $t/log
grep -q -- '-deployment_target_mismatches.*missing' $t/log
