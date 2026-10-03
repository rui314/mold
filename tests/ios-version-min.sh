#!/bin/bash
source "$(dirname "$0")"/common.inc

# ld-prime takes iOS's and Mac Catalyst's minimum versions as it does
# macOS's, and their older names with a note, given bare whatever -w
# says, that it reports on the option under its new name.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
sdk=$(xcrun --show-sdk-path)
link() { $mold -arch $ARCH -syslibroot "$sdk" -lSystem $t/a.o -o $t/exe "$@"; }

for opts in '-ios_version_min -ios_version_min' '-iphoneos_version_min -ios_version_min' \
  '-maccatalyst_version_min -maccatalyst_version_min' \
  '-iosmac_version_min -maccatalyst_version_min' \
  '-uikitformac_version_min -maccatalyst_version_min'; do
  set -- $opts
  opt=$1 new=$2

  not link -w $opt 2> $t/log
  grep -q -- "$new.*missing" $t/log
  not link -w $opt '' 2> $t/log
  grep -q -- "$new.*missing" $t/log
  not link -w $opt 1x 2> $t/log
  grep -q -- "$new: malformed 32-bit xxxx.yy.zz version number: '1x'" $t/log

  # Neither platform is one mold links for, which it says as it reads
  # the option. (ld-prime refuses the macOS object file.)
  not link -w $opt 17.0 2> $t/log
  if [ $opt != $new ]; then
    grep -q -- "^$opt has been renamed to $new$" $t/log
  else
    not grep -q 'renamed' $t/log
  fi
  if $mold -v 2> /dev/null | grep -q mold-macho; then
    grep -q 'unsupported platform: \(iOS\|macCatalyst\)$' $t/log
  fi
done
