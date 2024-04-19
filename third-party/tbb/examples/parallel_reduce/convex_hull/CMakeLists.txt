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

project(convex_hull_bench CXX)
project(convex_hull_sample CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

add_executable(convex_hull_bench convex_hull_bench.cpp)
add_executable(convex_hull_sample convex_hull_sample.cpp)

target_link_libraries(convex_hull_bench TBB::tbb Threads::Threads)
target_link_libraries(convex_hull_sample TBB::tbb Threads::Threads)

target_compile_options(convex_hull_bench PRIVATE ${TBB_CXX_STD_FLAG})
target_compile_options(convex_hull_sample PRIVATE ${TBB_CXX_STD_FLAG})

add_custom_target(convex_hull)
add_dependencies(convex_hull convex_hull_bench convex_hull_sample)

set(EXECUTABLE "$<TARGET_FILE:convex_hull_bench>")
set(PERF_EXECUTABLE "$<TARGET_FILE:convex_hull_sample>")
set(ARGS "")
set(LIGHT_ARGS 4 400)
set(PERF_ARGS silent auto 40000000)

add_execution_target(run_convex_hull convex_hull ${EXECUTABLE} "${ARGS}")
add_execution_target(light_test_convex_hull convex_hull ${EXECUTABLE} "${LIGHT_ARGS}")
add_execution_target(perf_run_convex_hull convex_hull ${PERF_EXECUTABLE} "${PERF_ARGS}")
