# Copyright (c) 2020-2021 Intel Corporation
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

project(sub_string_finder_simple CXX)
project(sub_string_finder_extended CXX)
project(sub_string_finder_pretty CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

add_executable(sub_string_finder_simple sub_string_finder.cpp)
add_executable(sub_string_finder_extended sub_string_finder_extended.cpp)
add_executable(sub_string_finder_pretty sub_string_finder_pretty.cpp)

target_link_libraries(sub_string_finder_simple TBB::tbb Threads::Threads)
target_link_libraries(sub_string_finder_extended TBB::tbb Threads::Threads)
target_link_libraries(sub_string_finder_pretty TBB::tbb Threads::Threads)

target_compile_options(sub_string_finder_simple PRIVATE ${TBB_CXX_STD_FLAG})
target_compile_options(sub_string_finder_extended PRIVATE ${TBB_CXX_STD_FLAG})
target_compile_options(sub_string_finder_pretty PRIVATE ${TBB_CXX_STD_FLAG})

add_custom_target(sub_string_finder)
add_dependencies(sub_string_finder sub_string_finder_simple sub_string_finder_extended sub_string_finder_pretty)

set(EXECUTABLE "$<TARGET_FILE:sub_string_finder_extended>")
set(LIGHT_EXECUTABLE "$<TARGET_FILE:sub_string_finder_simple>")
set(ARGS "")
set(LIGHT_ARGS "silent")

add_execution_target(run_sub_string_finder sub_string_finder ${EXECUTABLE} "${ARGS}")
add_execution_target(light_test_sub_string_finder sub_string_finder ${LIGHT_EXECUTABLE} "${LIGHT_ARGS}")
