#!/usr/bin/env bash
. $(dirname $0)/common.inc

cat <<EOF | $CC -c -o $t/data.o -xc -
char cafe[2] __attribute__((section(".cafe"))) = {42, 43};
char invalid[3] __attribute__((section(".invalid"))) = {44, 45, 46};
char plain[4] __attribute__((section("plain_name"))) = {47, 48, 49, 50};

extern char __start_caf__[], __stop_caf__[];
extern char __start_bad__[], __stop_bad__[];
extern char __start_plain_name[], __stop_plain_name[];
char *bounds[] = {
  __start_caf__, __stop_caf__, __start_bad__, __stop_bad__,
  __start_plain_name, __stop_plain_name,
};
EOF
$OBJCOPY --rename-section $'.cafe=.caf\xc3\xa9' \
  --rename-section $'.invalid=.bad\xe2\x82' $t/data.o

# Replace each byte of a UTF-8 character or an incomplete UTF-8 sequence
# with an underscore. Valid C identifiers retain their underscores.
./mold -o $t/exe $t/data.o -e cafe --start-stop
readelf -Ws $t/exe > $t/symbols
for name in caf__ bad__ plain_name; do
  grep " __start_$name\$" $t/symbols
  grep " __stop_$name\$" $t/symbols
done
