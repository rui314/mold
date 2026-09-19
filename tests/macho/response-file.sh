#!/usr/bin/env bash
. $(dirname $0)/common.inc

echo ' -help' > $t/rsp
$mold @$t/rsp | grep -q Usage
