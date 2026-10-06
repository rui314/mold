#!/usr/bin/env bash
. $(dirname $0)/common.inc

# Exercise several complete build-id hash shards and a partial final shard.
# The in-memory output path is an oracle for mapped-page advice: both paths
# must preserve every output byte, including the final build-id write.
cat <<EOF | $CC -c -o $t/a.o -xc -
const unsigned char payload[12 * 1024 * 1024 + 257] = {
  [0] = 0x12,
  [4 * 1024 * 1024] = 0x34,
  [8 * 1024 * 1024] = 0x56,
  [12 * 1024 * 1024 + 256] = 0x78,
};
const unsigned char *volatile data = payload;

int main(void) {
  return data[0] != 0x12 || data[4 * 1024 * 1024] != 0x34 ||
         data[8 * 1024 * 1024] != 0x56 ||
         data[12 * 1024 * 1024 + 256] != 0x78;
}
EOF

shard=$((4 * 1024 * 1024))
for hash in fast sha1 sha256; do
  for threads in 1 8; do
    $CC -B. -o $t/memory$threads $t/a.o \
      -Wl,--build-id=$hash,--threads=$threads,--no-mmap-output-file
    $CC -B. -o $t/mapped$threads $t/a.o \
      -Wl,--build-id=$hash,--threads=$threads,--mmap-output-file
    cmp $t/memory$threads $t/mapped$threads
  done

  size=$(wc -c < $t/memory1)
  test "$size" -gt "$((3 * shard))"
  test "$((size % shard))" -ne 0
  cmp $t/memory1 $t/memory8
  $QEMU $t/mapped8
done
