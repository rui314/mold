#!/bin/bash
. $(dirname $0)/common.inc

ldd mold-wrapper.so | grep -q libasan && skip
nm mold | grep -q '__[at]san_init' && skip

./mold -run bash -c 'echo $LD_PRELOAD' | grep -Fq mold-wrapper.so
