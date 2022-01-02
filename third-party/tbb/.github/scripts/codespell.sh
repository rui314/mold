#!/bin/bash
#
# Copyright (c) 2021 Intel Corporation
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

SCAN_TARGET=$1

SKIP_PATTERN='*/.github/*'

# Ignored cases
IGNORE_COMMAND="sed -e /.*\\sOd\\s*=.*/d \
-e /.*\\sOt\\s*=.*/d \
-e /.*\\siff\\s*=.*/d \
-e /.*\\sith\\s*=.*/d \
-e /.*\\scas\\s*=.*/d \
-e /.*\\sCAS\\s*=.*/d \
-e /.*\\ssom\\s*=.*/d \
-e /.*\\sSOM\\s*=.*/d \
-e /.*\\suint\\s*=.*/d \
-e /.*\\sUINT\\s*=.*/d \
-e /.*\\scopyable\\s*=.*/d \
-e /.*\\sCopyable\\s*=.*/d \
-e /.*\\sFo\\s*=.*/d \
-e /.*pipeline_filters.h.*nd\\s*=.*/d \
-e /.*ittnotify.h.*unx\\s*=.*/d \
-e /.*bzlib.cpp.*MSDOS\\s*=.*/d \
-e /.*test_task.cpp.*tE\\s*=.*/d \
-e /.*backend.cpp.*resSize\\s*=.*/d \
-e /.*test_join_node.h.*Ned\\s*=.*/d \
-e /.*test_indexer_node.cpp.*OT\\s*=.*/d \
-e /.*allocator_stl_test_common.h.*Aci*\\s*=.*/d \
-e /.*seismic_video.cpp.*DialogBox\\s*=.*/d \
-e /.*test_composite_node.cpp.*que\\s*=.*/d \
-e /.*blocksort.cpp.*hiSt\\s*=.*/d \
-e /.*compress.cpp.*fave\\s*=.*/d \
-e /.*count_strings.cpp.*ue\\s*=.*/d \
-e /.*count_strings.cpp.*nd\\s*=.*/d \
-e /.*count_strings.cpp.*ths\\s*=.*/d \
-e /.*polygon_overlay\/README.md.*ist\\s*=.*/d \
-e /.*_pipeline_filters.h.*nd\\s*=.*/d \
-e /.*sub_string_finder\/README.md.*ba\\s*=.*/d"

SCAN_RESULT=`codespell --quiet-level=2 --skip "${SKIP_PATTERN}" ${SCAN_TARGET}`
SCAN_RESULT=`echo -e "${SCAN_RESULT}" | ${IGNORE_COMMAND}`
echo "${SCAN_RESULT}"

if [[ ! -z ${SCAN_RESULT} ]]; then
    exit 1
fi
