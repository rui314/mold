#!/usr/bin/env bash
. $(dirname $0)/common.inc

raw=$'foo\xff'
replacement=$'foo\xef\xbf\xbd'
raw_alias=$'alias\xfe'

echo 'int original = 42; int replacement = 99;' |
  $CC -c -o $t/data.o -xc -
$OBJCOPY --redefine-sym "original=$raw" \
  --redefine-sym "replacement=$replacement" $t/data.o

cat <<EOF | $CC -c -o $t/main.o -xc -
extern int alias, raw_alias;
int main() { return alias != 42 || raw_alias != 42; }
EOF
$OBJCOPY --redefine-sym "raw_alias=$raw_alias" $t/main.o

# Both sides of a script assignment must retain their original bytes.
printf 'alias = "%s"; "%s" = "%s";\n' "$raw" "$raw_alias" "$raw" > $t/aliases.ld
$CC -B. -o $t/script $t/main.o $t/data.o -Wl,-T,$t/aliases.ld
$QEMU $t/script

# A --defsym target must also pull its defining file out of an archive.
ar crs $t/data.a $t/data.o
$CC -B. -o $t/argv $t/main.o $t/data.a \
  -Wl,--defsym=alias="$raw",--defsym="$raw_alias=$raw"
$QEMU $t/argv

echo 'int main() {}' | $CC -c -o $t/empty.o -xc -
for option in --undefined -u --require-defined; do
  $CC -B. -o $t/root $t/empty.o $t/data.a -Wl,$option,"$raw",--gc-sections
  nm $t/root > $t/symbols
  grep -aqF "$raw" $t/symbols
done

# Retaining an invalid UTF-8 name must not retain a different symbol whose
# name contains the Unicode replacement character.
printf ' \t\n\t%s \t' "$raw" > $t/keep
$CC -B. -o $t/retained $t/main.o $t/data.o \
  -Wl,-T,$t/aliases.ld,--retain-symbols-file=$t/keep
nm $t/retained > $t/symbols
grep -aqF "$raw" $t/symbols
not grep -aqF "$replacement" $t/symbols
$QEMU $t/retained
