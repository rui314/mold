#!/usr/bin/env bash
. $(dirname $0)/common.inc

./mold --version | grep -E '\(([0-9a-f]{40}; )?compatible with GNU ld\)'
