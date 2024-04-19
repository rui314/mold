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

project(primes CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

add_executable(primes main.cpp primes.cpp)

target_link_libraries(primes TBB::tbb Threads::Threads)
target_compile_options(primes PRIVATE ${TBB_CXX_STD_FLAG})

set(EXECUTABLE "$<TARGET_FILE:primes>")
set(ARGS "")
set(PERF_ARGS silent auto 1000000000 1000 20)

add_execution_target(run_primes primes ${EXECUTABLE} "${ARGS}")
add_execution_target(perf_run_primes primes ${EXECUTABLE} "${PERF_ARGS}")
