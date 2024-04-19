# Copyright (c) 2023 Intel Corporation
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

cmake_minimum_required(VERSION 3.1)

project(recursive_fibonacci CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

add_executable(recursive_fibonacci fibonacci.cpp)
target_link_libraries(recursive_fibonacci
                      TBB::tbb
                      Threads::Threads
                      $<$<PLATFORM_ID:Linux>:rt>)  # Link "rt" library on Linux
target_compile_options(recursive_fibonacci PRIVATE ${TBB_CXX_STD_FLAG})

set(EXECUTABLE "$<TARGET_FILE:recursive_fibonacci>")

# Parameters of executable N C I:
# `N` - specifies the fibonacci number which would be calculated.
# `C` - cutoff that will be used to stop recursive split.
# `I` - number of iteration to measure benchmark time.
set(ARGS 30 16 20)
set(PERF_ARGS 50 5 20)

add_execution_target(run_recursive_fibonacci recursive_fibonacci ${EXECUTABLE} "${ARGS}")
add_execution_target(perf_run_recursive_fibonacci recursive_fibonacci ${EXECUTABLE} "${PERF_ARGS}")
