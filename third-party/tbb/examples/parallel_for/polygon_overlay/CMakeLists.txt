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

project(polygon_overlay CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings("tbb;tbbmalloc")

add_executable(polygon_overlay pover_video.cpp polymain.cpp polyover.cpp)

add_subdirectory(../../common/gui gui)

target_link_libraries(polygon_overlay TBB::tbb TBB::tbbmalloc Threads::Threads UI_LIB_polygon_overlay)
target_compile_options(polygon_overlay PRIVATE ${TBB_CXX_STD_FLAG})
if (MSVC)
    target_compile_options(polygon_overlay PRIVATE -DNOMINMAX)
endif()

set(EXECUTABLE "$<TARGET_FILE:polygon_overlay>")
set(ARGS "")
set(LIGHT_ARGS --polys 10 --size 5x5)

add_execution_target(run_polygon_overlay polygon_overlay ${EXECUTABLE} "${ARGS}")
add_execution_target(light_test_polygon_overlay polygon_overlay ${EXECUTABLE} "${LIGHT_ARGS}")

