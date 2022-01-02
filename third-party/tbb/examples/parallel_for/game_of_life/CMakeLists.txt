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

project(game_of_life CXX)

include(../../common/cmake/common.cmake)

set_common_project_settings(tbb)

add_executable(game_of_life Evolution.cpp Game_of_life.cpp Update_state.cpp)

target_compile_definitions(game_of_life PUBLIC _CONSOLE)
target_link_libraries(game_of_life TBB::tbb Threads::Threads)
target_compile_options(game_of_life PRIVATE ${TBB_CXX_STD_FLAG})

set(EXECUTABLE "$<TARGET_FILE:game_of_life>")
set(ARGS 2:4 -t 5)
set(LIGHT_ARGS 1:2 -t 5)

add_execution_target(run_game_of_life game_of_life ${EXECUTABLE} "${ARGS}")
add_execution_target(light_test_game_of_life game_of_life ${EXECUTABLE} "${LIGHT_ARGS}")
