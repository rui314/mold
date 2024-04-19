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

project(dining_philosophers CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

add_executable(dining_philosophers dining_philosophers.cpp)

target_link_libraries(dining_philosophers TBB::tbb Threads::Threads)
target_compile_options(dining_philosophers PRIVATE ${TBB_CXX_STD_FLAG})

set(EXECUTABLE "$<TARGET_FILE:dining_philosophers>")
set(ARGS auto 5)
set(LIGHT_ARGS auto 3)

add_execution_target(run_dining_philosophers dining_philosophers ${EXECUTABLE} "${ARGS}")
add_execution_target(light_test_dining_philosophers dining_philosophers ${EXECUTABLE} "${LIGHT_ARGS}")
