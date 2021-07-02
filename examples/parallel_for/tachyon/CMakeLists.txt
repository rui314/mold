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

project(tachyon CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

set(TACHYON_VERSION tbb CACHE STRING "Defines the version of the tachyon example")

add_executable(
    tachyon src/trace.${TACHYON_VERSION}.cpp
    src/main.cpp src/pthread.cpp
    src/tachyon_video.cpp src/api.cpp
    src/apigeom.cpp
    src/apitrigeom.cpp
    src/bndbox.cpp
    src/box.cpp
    src/camera.cpp
    src/coordsys.cpp
    src/cylinder.cpp
    src/extvol.cpp
    src/global.cpp
    src/grid.cpp
    src/imageio.cpp
    src/imap.cpp
    src/intersect.cpp
    src/jpeg.cpp
    src/light.cpp
    src/objbound.cpp
    src/parse.cpp
    src/plane.cpp
    src/ppm.cpp
    src/quadric.cpp
    src/render.cpp
    src/ring.cpp
    src/shade.cpp
    src/sphere.cpp
    src/texture.cpp
    src/tgafile.cpp
    src/trace_rest.cpp
    src/triangle.cpp
    src/ui.cpp
    src/util.cpp
    src/vector.cpp
    src/vol.cpp 
)

add_subdirectory(../../common/gui gui)

target_link_libraries(tachyon TBB::tbb Threads::Threads UI_LIB_tachyon)
target_compile_options(tachyon PRIVATE ${TBB_CXX_STD_FLAG})

if (MSVC AND CMAKE_CXX_COMPILER_ID STREQUAL IntelLLVM)
    target_compile_options(tachyon PRIVATE -D_CRT_SECURE_NO_WARNINGS)
endif()

set(EXECUTABLE "$<TARGET_FILE:tachyon>")
set(ARGS ${CMAKE_CURRENT_SOURCE_DIR}/dat/balls.dat)
set(PERF_ARGS silent ${CMAKE_CURRENT_SOURCE_DIR}/dat/balls3.dat)
set(LIGHT_ARGS ${CMAKE_CURRENT_SOURCE_DIR}/dat/model2.dat)

add_execution_target(run_tachyon tachyon ${EXECUTABLE} "${ARGS}")
add_execution_target(perf_run_tachyon tachyon ${EXECUTABLE} "${PERF_ARGS}")
add_execution_target(light_test_tachyon tachyon ${EXECUTABLE} "${LIGHT_ARGS}")
