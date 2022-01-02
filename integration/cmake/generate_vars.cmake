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

# Reuired parameters:
#     SOURCE_DIR       - incoming path to oneTBB source directory.
#     BINARY_DIR       - incoming path to oneTBB build directory.
#     BIN_PATH         - incoming path to oneTBB binaries directory.
#     TBB_INSTALL_VARS - install vars generation trigger
#     TBB_CMAKE_INSTALL_LIBDIR - subdir for shared object files installation path (used only in TBB_INSTALL_VARS mode)
#     VARS_TEMPLATE    - path to the vars template file
#     VARS_NAME        - name of the output vars script

set(INPUT_FILE "${SOURCE_DIR}/integration/${VARS_TEMPLATE}")
set(OUTPUT_FILE "${BIN_PATH}/${VARS_NAME}")

file(TO_NATIVE_PATH "${SOURCE_DIR}" TBBROOT_REPLACEMENT)
file(TO_NATIVE_PATH "${BIN_PATH}" LIBRARY_PATH_REPLACEMENT)
if (WIN32)
    file(TO_NATIVE_PATH "${BIN_PATH}" BINARY_PATH_REPLACEMENT)
endif()

if (NOT EXISTS ${OUTPUT_FILE})
    configure_file(${INPUT_FILE} ${OUTPUT_FILE} @ONLY)
endif()

if (TBB_INSTALL_VARS)
    set(OUTPUT_FILE "${BINARY_DIR}/internal_install_vars")
    if (UNIX)
        set(TBBROOT_REPLACEMENT "$(cd $(dirname \${BASH_SOURCE}) && pwd -P)/..")
        set(LIBRARY_PATH_REPLACEMENT "$TBBROOT/${TBB_CMAKE_INSTALL_LIBDIR}/")
        set(CMAKE_ENVIRONMENT_SOURCING_STRING "CMAKE_PREFIX_PATH=\"\${TBBROOT}/${TBB_CMAKE_INSTALL_LIBDIR}/cmake/TBB:${CMAKE_PREFIX_PATH}\"; export CMAKE_PREFIX_PATH")
    else()
        set(TBBROOT_REPLACEMENT "%~d0%~p0..")
        set(LIBRARY_PATH_REPLACEMENT "%TBBROOT%\\${TBB_CMAKE_INSTALL_LIBDIR}")
        set(BINARY_PATH_REPLACEMENT "%TBBROOT%\\bin")
        set(CMAKE_ENVIRONMENT_SOURCING_STRING "set \"CMAKE_PREFIX_PATH=%TBBROOT%\\${TBB_CMAKE_INSTALL_LIBDIR}\\cmake\\TBB;%CMAKE_PREFIX_PATH%\"")
    endif()

    configure_file( ${INPUT_FILE} ${OUTPUT_FILE} @ONLY )
endif()
