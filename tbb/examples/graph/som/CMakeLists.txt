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

include(../../common/cmake/common.cmake)
project(som CXX)
set_common_project_settings(tbb)

add_executable(som som_graph.cpp som.cpp)

target_link_libraries(som TBB::tbb Threads::Threads)
target_compile_options(som PRIVATE ${TBB_CXX_STD_FLAG})

set(EXECUTABLE "$<TARGET_FILE:som>")
set(ARGS)
set(LIGHT_ARGS 4)

add_execution_target(run_som som ${EXECUTABLE} "${ARGS}")
add_execution_target(light_test_som som ${EXECUTABLE} "${LIGHT_ARGS}")
