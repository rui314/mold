#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<'EOF' | $CC -c -ffunction-sections -o $t/a.o -xc -
__attribute__((noinline)) int foo(void) { return 42; }
__attribute__((noinline)) int bar(void) { return 42; }
void _start(void) { foo(); bar(); }
EOF

$CC -B. -nostdlib -Wl,--stats -Wl,--icf=all -Wl,-e,_start -o $t/exe $t/a.o > $t/log

grep -E '^ *parsed_objs=[1-9][0-9]*$' $t/log
grep -E '^ *all_syms=[1-9][0-9]*$' $t/log
grep -E '^ *regular_sections=[1-9][0-9]*$' $t/log
grep -E '^ *total_input_bytes=[1-9][0-9]*$' $t/log
grep -E '^ *icf_eligibles=[1-9][0-9]*$' $t/log
grep -E '^ *icf_round=[1-9][0-9]*$' $t/log
