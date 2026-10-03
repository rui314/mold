#!/bin/bash
source "$(dirname "$0")"/common.inc

# The options that have ld-prime report on its own workings (its branch
# islands, order file statistics, the libraries re-exports load, the
# section each symbol goes to, a link snapshot, the reference graph,
# the files and symbols it used) leave the output as it is. Those that
# name a file or a name want one; -debug_snapshot takes a mode after
# its name, and -max_code_deduplicate_passes a decimal number.
echo 'int main() { return 0; }' | $CC -o $t/a.o -c -xc -
sdk=$(xcrun --show-sdk-path)
link() {
  $mold -arch $ARCH -platform_version macos 26.0 26.0 -syslibroot "$sdk" -lSystem $t/a.o \
    -o $t/exe "$@"
}

link
cp $t/exe $t/exe0

for opt in -verbose_branch_islands -order_file_statistics -trace_implicit_libraries \
  -trace_symbol_layout -no_snapshot -arch_multiple -no_warn_eh_frame_too_large; do
  link $opt > /dev/null 2> $t/log
  not grep -q 'warning\|error' $t/log
  cmp $t/exe $t/exe0
done

for opt in -trace_symbol_layout_file -dot -trace_file -trace_file_shared_cache \
  -trace_symbols_file; do
  link $opt $t/report 2> $t/log
  cmp $t/exe $t/exe0
  not link $opt '' 2> $t/log
  grep -q -- "$opt.*missing" $t/log
done
link -trace_implicit_library libsystem_c.dylib > /dev/null
cmp $t/exe $t/exe0
not link -trace_implicit_library 2> $t/log
grep -q -- '-trace_implicit_library.*missing' $t/log
not link -snapshot_dir 2> $t/log
grep -q -- '-snapshot_dir.*missing' $t/log
not link -reference_output 2> $t/log
grep -q -- '-reference_output.*missing' $t/log

mkdir -p $t/snap
for opt in -debug_snapshot -debug_snapshot= -debug_snapshot=minimal -debug_snapshotminimal; do
  link $opt -snapshot_dir $t/snap
  cmp $t/exe $t/exe0
done
not link -debug_snapshot=full 2> $t/log
grep -q 'unknown debug snapshot mode: full$' $t/log
not link -debug_snapshot_minimal 2> $t/log
grep -q 'unknown debug snapshot mode: _minimal$' $t/log

for n in 0 3 ' 3' +3 -3; do
  link -max_code_deduplicate_passes "$n"
done
for n in 0x3 '3 ' x; do
  not link -max_code_deduplicate_passes "$n" 2> $t/log
  grep -q 'invalid argument for -max_code_deduplicate_passes' $t/log
done
not link -max_code_deduplicate_passes '' 2> $t/log
grep -q -- '-max_code_deduplicate_passes.*missing' $t/log

# -x86_64_layout_emulation is for arm64 links: another gets a warning
# once every option is read, before the obsolete options'.
link -x86_64_layout_emulation -X 2> $t/log
if [ $ARCH = arm64 ]; then
  not grep -q "ignoring -x86_64_layout_emulation" $t/log
else
  grep -q 'warning: ignoring -x86_64_layout_emulation option, it can only be used with -arch arm64' \
    $t/log
  [ "$(grep -n layout_emulation $t/log | tail -1 | cut -d: -f1)" -lt \
    "$(grep -n 'is obsolete' $t/log | cut -d: -f1)" ]
fi
