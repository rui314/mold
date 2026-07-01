#!/usr/bin/env bash
. $(dirname $0)/common.inc

# A section whose name is a valid C identifier should be GC'd if
# neither its contents nor its __start_/__stop_ marker symbols are
# referenced from a live section.
cat <<EOF | $CC -c -o $t/a.o -xc -
__attribute__((section("foo"))) int dead_data = 42;
__attribute__((section("bar"))) int live_data = 99;

extern int __start_bar[];
extern int __stop_bar[];

int main() {
  return __stop_bar - __start_bar + live_data;
}
EOF

$CC -B. -o $t/exe $t/a.o -Wl,-gc-sections
readelf --symbols $t/exe > $t/log

not grep dead_data $t/log
grep live_data $t/log
grep __start_bar $t/log
grep __stop_bar $t/log
