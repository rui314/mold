#!/bin/bash
. $(dirname $0)/common.inc

ldd mold-wrapper.so | grep libasan && skip
nm mold | grep '__[at]san_init' && skip

./mold -run bash -c 'echo $LD_PRELOAD' | grep -F mold-wrapper.so
