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

list(APPEND HWLOC_REQUIRED_VERSIONS 1_11 2 2_5)

foreach(hwloc_version ${HWLOC_REQUIRED_VERSIONS})
    if (NOT WIN32)
        set(CMAKE_HWLOC_${hwloc_version}_DLL_PATH STUB)
    endif()
    set(HWLOC_TARGET_NAME HWLOC::hwloc_${hwloc_version})

    if (NOT TARGET ${HWLOC_TARGET_NAME} AND
        CMAKE_HWLOC_${hwloc_version}_LIBRARY_PATH AND
        CMAKE_HWLOC_${hwloc_version}_DLL_PATH AND
        CMAKE_HWLOC_${hwloc_version}_INCLUDE_PATH
    )
        add_library(${HWLOC_TARGET_NAME} SHARED IMPORTED)
        set_target_properties(${HWLOC_TARGET_NAME} PROPERTIES INTERFACE_INCLUDE_DIRECTORIES
            "${CMAKE_HWLOC_${hwloc_version}_INCLUDE_PATH}")
        if (WIN32)
            set_target_properties(${HWLOC_TARGET_NAME} PROPERTIES
                                  IMPORTED_LOCATION "${CMAKE_HWLOC_${hwloc_version}_DLL_PATH}"
                                  IMPORTED_IMPLIB   "${CMAKE_HWLOC_${hwloc_version}_LIBRARY_PATH}")
        else()
            set_target_properties(${HWLOC_TARGET_NAME} PROPERTIES
                                  IMPORTED_LOCATION "${CMAKE_HWLOC_${hwloc_version}_LIBRARY_PATH}")
        endif()
    endif()

    if (TARGET ${HWLOC_TARGET_NAME})
        set(HWLOC_TARGET_EXPLICITLY_DEFINED TRUE)
    endif()
endforeach()

unset(HWLOC_TARGET_NAME)

if (NOT HWLOC_TARGET_EXPLICITLY_DEFINED AND
    NOT TBB_DISABLE_HWLOC_AUTOMATIC_SEARCH
)
    find_package(PkgConfig QUIET)
    if (PKG_CONFIG_FOUND AND NOT CMAKE_VERSION VERSION_LESS 3.6)
        pkg_search_module(HWLOC hwloc IMPORTED_TARGET)
        if (TARGET PkgConfig::HWLOC)
            if (HWLOC_VERSION VERSION_LESS 2)
                set(TBBBIND_LIBRARY_NAME tbbbind)
            elseif(HWLOC_VERSION VERSION_LESS 2.5)
                set(TBBBIND_LIBRARY_NAME tbbbind_2_0)
            else()
                set(TBBBIND_LIBRARY_NAME tbbbind_2_5)
            endif()
        endif()
    endif()
endif()
