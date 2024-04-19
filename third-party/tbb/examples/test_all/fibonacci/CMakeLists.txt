# Copyright (c) 2019-2021 Intel Corporation
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

project(fibonacci CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

add_executable(fibonacci fibonacci.cpp)
target_link_libraries(fibonacci
                      TBB::tbb
                      Threads::Threads
                      $<$<PLATFORM_ID:Linux>:rt>)  # Link "rt" library on Linux
target_compile_options(fibonacci PRIVATE ${TBB_CXX_STD_FLAG})

set(EXECUTABLE "$<TARGET_FILE:fibonacci>")
set(ARGS "")

add_execution_target(run_fibonacci fibonacci ${EXECUTABLE} "${ARGS}")
