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

option(TBB_INSTALL_VARS "Enable auto-generated vars installation" OFF)

if (WIN32)
    set(TBB_VARS_TEMPLATE "windows/env/vars.bat.in")
elseif (APPLE)
    set(TBB_VARS_TEMPLATE "mac/env/vars.sh.in")
else()
    set(TBB_VARS_TEMPLATE "linux/env/vars.sh.in")
endif()

get_filename_component(TBB_VARS_TEMPLATE_NAME ${PROJECT_SOURCE_DIR}/integration/${TBB_VARS_TEMPLATE} NAME)
string(REPLACE ".in" "" TBB_VARS_NAME ${TBB_VARS_TEMPLATE_NAME})

macro(tbb_gen_vars target)
    if (${CMAKE_PROJECT_NAME} STREQUAL ${PROJECT_NAME})
        add_custom_command(TARGET ${target} POST_BUILD COMMAND
            ${CMAKE_COMMAND}
            -DBINARY_DIR=${CMAKE_BINARY_DIR}
            -DSOURCE_DIR=${PROJECT_SOURCE_DIR}
            -DBIN_PATH=$<TARGET_FILE_DIR:${target}>
            -DVARS_TEMPLATE=${TBB_VARS_TEMPLATE}
            -DVARS_NAME=${TBB_VARS_NAME}
            -DTBB_INSTALL_VARS=${TBB_INSTALL_VARS}
            -DTBB_CMAKE_INSTALL_LIBDIR=${CMAKE_INSTALL_LIBDIR}
            -P ${PROJECT_SOURCE_DIR}/integration/cmake/generate_vars.cmake
        )
    endif()
endmacro(tbb_gen_vars)

if (TBB_INSTALL_VARS)
    install(PROGRAMS "${CMAKE_BINARY_DIR}/internal_install_vars"
            DESTINATION env
            RENAME ${TBB_VARS_NAME})
endif()
