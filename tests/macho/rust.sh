#!/usr/bin/env bash
. $(dirname $0)/common.inc

command -v rustc >/dev/null || skip

cat <<EOF2 > $t/main.rs
use std::collections::HashMap;
fn main() {
    let mut m = HashMap::new();
    m.insert("hello", 42);
    let v: Vec<i32> = (1..=10).collect();
    let sum: i32 = v.iter().sum();
    println!("rust says {} {}", m["hello"], sum);
    let r = std::panic::catch_unwind(|| panic!("boom"));
    println!("caught panic: {}", r.is_err());
}
EOF2

rustc -C link-arg=--ld-path=$mold $t/main.rs -o $t/exe
$t/exe 2>/dev/null > $t/log
grep 'rust says 42 55' $t/log
grep 'caught panic: true' $t/log
