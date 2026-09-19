#!/usr/bin/env bash
. $(dirname $0)/common.inc

command -v swiftc >/dev/null || skip
# swiftc drives the link itself; there is no per-arch cc wrapper here.
[ "$ARCH" = "$(uname -m)" ] || skip

cat <<EOF2 > $t/main.swift
struct Point { var x: Int; var y: Int }
let p = Point(x: 3, y: 4)
let arr = [1, 2, 3].map { \$0 * 2 }
print("swift \(p.x + p.y) \(arr.reduce(0, +))")
EOF2

swiftc -o $t/exe $t/main.swift -use-ld=$mold
$t/exe | grep 'swift 7 12'
