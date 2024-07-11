# Copyright (c) 2020-2023 Intel Corporation
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

cmake_minimum_required(VERSION 3.5)

project(count_strings CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

add_executable(count_strings count_strings.cpp)

target_link_libraries(count_strings TBB::tbb Threads::Threads)
target_compile_options(count_strings PRIVATE ${TBB_CXX_STD_FLAG})

set(EXECUTABLE "$<TARGET_FILE:count_strings>")
set(ARGS "")
set(PERF_ARGS auto 10000000 silent)

add_execution_target(run_count_strings count_strings ${EXECUTABLE} "${ARGS}")
add_execution_target(perf_run_count_strings count_strings ${EXECUTABLE} "${PERF_ARGS}")
