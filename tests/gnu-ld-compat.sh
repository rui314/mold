#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -o $t/a.o -c -xc -
int main() {}
EOF

# These spellings need the winnow-args parser (--features winnow-args);
# the built-in parser does not take them yet.
./mold -max-cache-size=1 $t/a.o > $t/probe 2>&1 || true
if ! grep -q 'unknown -m argument' $t/probe; then
  skip
fi

# GNU ld's dash rules: a long option spelled with one dash is read as the
# long option, not as a short option with the rest of its name as a value.

# -entry=main is --entry=main, not -e ntry=main: the entry point is main.
entry_of() {
  readelf -h $1 | awk '/Entry point address/ {print $4}'
}
main_of() {
  printf '0x%x' "$(($(nm $1 | awk '$3 == "main" {print "16#" $1}')))"
}

$CC -B. -o $t/exe1 -Wl,-entry=main $t/a.o
test "$(entry_of $t/exe1)" = "$(main_of $t/exe1)"

# -emain is -e main.
$CC -B. -o $t/exe2 -Wl,-emain $t/a.o
test "$(entry_of $t/exe2)" = "$(main_of $t/exe2)"

# -Ttext=0x1000 sets the text section address, as --Ttext=0x1000 does.
$CC -B. -o $t/exe3 -Wl,-Ttext=0x1000 $t/a.o
readelf -SW $t/exe3 | grep -E '\.text\s+PROGBITS\s+0000000000001000' > /dev/null

# Where GNU ld reads the single-dash spelling as a short option with an
# attached value: -export-dynamic-symbol is -e xport-dynamic-symbol, so foo
# is an input file, and -max-cache-size=1 is -m ax-cache-size=1.
not ./mold -export-dynamic-symbol foo $t/a.o |& grep 'cannot open foo'
not ./mold -max-cache-size=1 $t/a.o |& grep 'unknown -m argument: ax-cache-size=1'

# -omagic is -o magic, as in GNU ld.
(cd $t && $OLDPWD/mold -omagic a.o)
test -f $t/magic

# GNU ld's short aliases, and the options GNU ld accepts and ignores.

# -i is an alias for -r, so it makes a relocatable output.
$CC -B. -o $t/exe4 -Wl,-i $t/a.o
readelf -h $t/exe4 > $t/log4
grep 'Type:.*REL ' $t/log4

# -U and -Ur are -r as well: GNU ld makes a relocatable output.
$CC -B. -o $t/exe -Wl,-U $t/a.o
readelf -h $t/exe > $t/log
grep 'Type:.*REL ' $t/log
$CC -B. -o $t/exe -Wl,-Ur $t/a.o
readelf -h $t/exe > $t/log
grep 'Type:.*REL ' $t/log
not ./mold -Ufoo $t/a.o |& grep 'unknown command line option: -Ufoo'
not ./mold -U r $t/a.o |& grep 'cannot open r'

# -n does not page-align data, so the result is not run; -t traces inputs.
$CC -B. -o $t/exe5 -Wl,-n $t/a.o
$CC -B. -o $t/exe6 -Wl,-t $t/a.o > $t/log6
grep $t/a.o $t/log6

# A short option that GNU ld ignores, and one that forces common symbols
# to be defined.
$CC -B. -o $t/exe7 -Wl,-g $t/a.o
$CC -B. -o $t/exe8 -Wl,-d $t/a.o

# -a and -c take their value attached as well as in a separate word, so
# they must not be confused with the longer options that start with the
# same letter.
$CC -B. -o $t/exe9 -Wl,-a -Wl,shared -Wl,-ashared $t/a.o
$CC -B. -o $t/exe10 -Wl,-auxiliary -Wl,$t/a.o -Wl,-shared
$CC -B. -o $t/exe11 -Wl,--as-needed $t/a.o
$CC -B. -o $t/exe12 -Wl,--compress-debug-sections=zlib $t/a.o
$CC -B. -o $t/exe13 -Wl,-assert -Wl,pure-text $t/a.o
$CC -B. -o $t/exe14 -Wl,-Y -Wl,$t $t/a.o

# GNU ld rejects an -a or -assert keyword it does not know, and the '='
# forms, which would abbreviate several long options.
not ./mold -a bogus $t/a.o |& grep "unrecognized -a option .bogus"
not ./mold -a=shared $t/a.o |& grep "unrecognized -a option .=shared"
not ./mold -assert bogus $t/a.o |& grep "unrecognized -assert option .bogus"

# Vendor-specific spellings mold has no use for.
$CC -B. -o $t/exe16 -Wl,-Qy $t/a.o
$CC -B. -o $t/exe17 -Wl,-A -Wl,x86-64 $t/a.o
$CC -B. -o $t/exe18 -Wl,-G -Wl,8 $t/a.o
$CC -B. -o $t/exe19 -Wl,-dT -Wl,$t/nosuchscript $t/a.o
$CC -B. -o $t/exe20 -Wl,-c -Wl,$t/nosuchscript $t/a.o

# The long names the short options stand for.
$CC -B. -o $t/exe21 -Wl,--architecture -Wl,x86-64 $t/a.o
$CC -B. -o $t/exe22 -Wl,--gpsize -Wl,8 $t/a.o
$CC -B. -o $t/exe23 -Wl,--mri-script -Wl,$t/nosuchscript $t/a.o
$CC -B. -o $t/exe24 -Wl,--default-script -Wl,$t/nosuchscript $t/a.o

# A name that merely starts like -a is read as -a with the rest of the
# name as its keyword, which GNU ld rejects.
not ./mold -auxiliaries |& grep "unrecognized -a option .uxiliaries"
not ./mold -a KEYWORD |& grep "unrecognized -a option .KEYWORD"

# GNU ld rewrites every "-lfoo" to "--library=foo" before parsing, so no
# long option starting with "l" is ever read with one dash, and a "-G"
# that names no size becomes "--shared".
not ./mold -library-path $t/a.o |& grep 'library not found: ibrary-path'
not ./mold -lto-pseudo-probe-for-profiling $t/a.o |& grep 'library not found: to-pseudo-probe-for-profiling'
not ./mold -G foo $t/a.o |& grep 'cannot open foo'

# GNU ld reads "-architecture" as "-a rchitecture" and "-mri-script" as
# "-m ri-script", so those long names need two dashes.
not ./mold -architecture x86-64 $t/a.o |& grep "unrecognized -a option .rchitecture"
not ./mold -mri-script foo $t/a.o |& grep 'unknown -m argument: ri-script'

# Options whose value GNU ld makes optional: accepted bare, and with the
# value attached by an equal sign.
$CC -B. -o $t/exe25 -Wl,--verbose $t/a.o
$CC -B. -o $t/exe26 -Wl,--verbose=3 $t/a.o
$CC -B. -o $t/exe27 -Wl,--sort-common $t/a.o
$CC -B. -o $t/exe28 -Wl,--sort-common=descending $t/a.o
$CC -B. -o $t/exe29 -Wl,--demangle $t/a.o
$CC -B. -o $t/exe30 -Wl,--demangle=gnu-v3 $t/a.o
$CC -B. -o $t/exe -Wl,--demangle=none $t/a.o
$CC -B. -o $t/exe31 -Wl,--fix-cortex-a53-843419=adr $t/a.o
$CC -B. -o $t/exe32 -Wl,--split-by-file=4096 $t/a.o
$CC -B. -o $t/exe33 -Wl,--split-by-reloc=10 $t/a.o
$CC -B. -o $t/exe34 -Wl,--orphan-handling=place $t/a.o
$CC -B. -o $t/exe -Wl,--orphan-handling=warn $t/a.o
$CC -B. -o $t/exe35 -Wl,--orphan-handling -Wl,place $t/a.o
$CC -B. -o $t/exe36 -Wl,--no-stats $t/a.o
$CC -B. -o $t/exe -Wl,--no-stats=1 $t/a.o
$CC -B. -o $t/exe -Wl,--sort-common=ascending $t/a.o

# GNU ld reads the values of the options whose value it makes optional,
# and rejects the ones it does not know.
not ./mold --demangle=gnu $t/a.o |& grep "unknown demangling style .gnu"
not ./mold --sort-common=bogus $t/a.o |& grep 'invalid common section sorting option: bogus'
not ./mold --orphan-handling=bogus $t/a.o |& grep 'invalid argument to option "--orphan-handling"'
not ./mold --orphan-handling=script $t/a.o |& grep 'invalid argument to option "--orphan-handling"'
not ./mold --verbose=bogus $t/a.o |& grep 'invalid number .bogus'

# A name that merely starts like an option is still unknown.
not ./mold --sort-commonplace |& grep 'unknown command line option: --sort-commonplace'

# GNU ld's informational and no-op options: accepted and ignored.
for opt in --print-map-discarded --no-print-map-discarded --print-map-locals \
  --no-print-map-locals --strip-discarded --no-strip-discarded --map-whole-files \
  --no-map-whole-files --cref --print-memory-usage --print-sysroot \
  --print-output-format --target-help --force-exe-suffix --traditional-format \
  --qmagic --reduce-memory-overheads --accept-unknown-input-arch \
  --no-accept-unknown-input-arch --no-warn-mismatch --no-warn-search-mismatch \
  --force-group-allocation --enable-non-contiguous-regions \
  --enable-non-contiguous-regions-warnings --disable-linker-version \
  --enable-linker-version --no-enum-size-warning --no-wchar-size-warning \
  --default-imported-symver --warn-execstack-objects --warn-section-align \
  --warn-multiple-gp --warn-alternate-em --error-execstack --warn-rwx-segments \
  --error-rwx-segments --no-define-common --dynamic-list-cpp-new \
  --dynamic-list-cpp-typeinfo --check-sections --no-check-sections; do
  $CC -B. -o $t/exe -Wl,$opt $t/a.o
done

# The ones that take a value, separately or attached by an equal sign.
for opt in --hash-size=1024 --remap-inputs=a=b --error-handling-script=$t/err.sh \
  --version-exports-section=VER; do
  $CC -B. -o $t/exe -Wl,$opt $t/a.o
done
$CC -B. -o $t/exe -Wl,--hash-size -Wl,2048 $t/a.o
$CC -B. -o $t/exe -Wl,--remap-inputs-file -Wl,$t/remap.txt $t/a.o

# Single-dash spellings of long options that start with a short option's
# letter: -qmagic is --qmagic, not -q magic, and -hash-size=1024 is
# --hash-size=1024, not -h ash-size=1024.
$CC -B. -o $t/exe -Wl,-qmagic $t/a.o
$CC -B. -o $t/exe -Wl,-hash-size=1024 $t/a.o
$CC -B. -o $t/exe -Wl,-hash-size -Wl,2048 $t/a.o

# Several short flags in one word: accepted, with GNU ld's deprecation
# warning, as -s -S. -s strips the symbol table.
$CC -B. -o $t/exe -Wl,-sS $t/a.o 2> $t/log
grep 'grouped short command line options are deprecated: -sS' $t/log
test -z "$(readelf -SW $t/exe | grep '\.symtab')"

$CC -B. -o $t/exe -Wl,-sx $t/a.o 2> $t/log
grep 'grouped short command line options are deprecated: -sx' $t/log

# -sr is -s -r, so it makes a relocatable output; but GNU ld reads -r and
# -i as a word of their own, so -rs and -is are unknown.
$CC -B. -o $t/exe -Wl,-sr $t/a.o 2> $t/log
grep 'grouped short command line options are deprecated: -sr' $t/log
readelf -h $t/exe > $t/log2
grep 'Type:.*REL ' $t/log2
not ./mold -rs $t/a.o |& grep 'unrecognised option: -rs'
not ./mold -is $t/a.o |& grep 'unrecognised option: -is'

# A letter that takes a value ends the bundle, and GNU ld rejects the
# word, so it stays unknown.
not ./mold -sO2 $t/a.o |& grep 'unknown command line option: -sO2'

# A word that names a long option is not a bundle: -init is --init, not
# -i -n -i -t, so main is the init symbol, not an input file.
$CC -B. -o $t/exe -Wl,-init,main $t/a.o

# -z keywords GNU ld accepts and mold has no use for.
for kw in global globalaudit loadfltr start-stop-gc nostart-stop-gc unique nounique \
  unique-symbol nounique-symbol; do
  $CC -B. -o $t/exe -Wl,-z -Wl,$kw $t/a.o
done
$CC -B. -o $t/exe -Wl,-zglobal $t/a.o
