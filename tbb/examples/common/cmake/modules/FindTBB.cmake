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

include(FindPackageHandleStandardArgs)

# Firstly search for TBB in config mode (i.e. search for TBBConfig.cmake).
find_package(TBB QUIET CONFIG)
if (TBB_FOUND)
    find_package_handle_standard_args(TBB CONFIG_MODE)
    return()
endif()

if (NOT TBB_FIND_COMPONENTS)
    set(TBB_FIND_COMPONENTS tbb tbbmalloc)
    foreach (_tbb_component ${TBB_FIND_COMPONENTS})
        set(TBB_FIND_REQUIRED_${_tbb_component} 1)
    endforeach()
endif()

if (WIN32)
    list(APPEND ADDITIONAL_LIB_DIRS ENV PATH ENV LIB)
    list(APPEND ADDITIONAL_INCLUDE_DIRS ENV INCLUDE ENV CPATH)
else()
    list(APPEND ADDITIONAL_LIB_DIRS ENV LIBRARY_PATH ENV LD_LIBRARY_PATH ENV DYLD_LIBRARY_PATH)
    list(APPEND ADDITIONAL_INCLUDE_DIRS ENV CPATH ENV C_INCLUDE_PATH ENV CPLUS_INCLUDE_PATH ENV INCLUDE_PATH)
endif()

find_path(_tbb_include_dir NAMES tbb/tbb.h PATHS ${ADDITIONAL_INCLUDE_DIRS})

if (_tbb_include_dir)
    # TODO: consider TBB_VERSION handling
    set(_TBB_BUILD_MODES RELEASE DEBUG)
    set(_TBB_DEBUG_SUFFIX _debug)

    foreach (_tbb_component ${TBB_FIND_COMPONENTS})
        if (NOT TARGET TBB::${_tbb_component})
            add_library(TBB::${_tbb_component} SHARED IMPORTED)
            set_property(TARGET TBB::${_tbb_component} APPEND PROPERTY INTERFACE_INCLUDE_DIRECTORIES ${_tbb_include_dir})

            foreach(_TBB_BUILD_MODE ${_TBB_BUILD_MODES})
                set(_tbb_component_lib_name ${_tbb_component}${_TBB_${_TBB_BUILD_MODE}_SUFFIX})
                if (WIN32)
                    find_library(${_tbb_component_lib_name}_lib ${_tbb_component_lib_name} PATHS ${ADDITIONAL_LIB_DIRS})
                    find_file(${_tbb_component_lib_name}_dll ${_tbb_component_lib_name}.dll PATHS ${ADDITIONAL_LIB_DIRS})

                    set_target_properties(TBB::${_tbb_component} PROPERTIES
                                          IMPORTED_LOCATION_${_TBB_BUILD_MODE} "${${_tbb_component_lib_name}_dll}"
                                          IMPORTED_IMPLIB_${_TBB_BUILD_MODE}   "${${_tbb_component_lib_name}_lib}"
                                          )
                else()
                    find_library(${_tbb_component_lib_name}_so ${_tbb_component_lib_name} PATHS ${ADDITIONAL_LIB_DIRS})

                    set_target_properties(TBB::${_tbb_component} PROPERTIES
                                          IMPORTED_LOCATION_${_TBB_BUILD_MODE} "${${_tbb_component_lib_name}_so}"
                                          )
                endif()
                if (${_tbb_component_lib_name}_lib AND ${_tbb_component_lib_name}_dll OR ${_tbb_component_lib_name}_so)
                    set_property(TARGET TBB::${_tbb_component} APPEND PROPERTY IMPORTED_CONFIGURATIONS ${_TBB_BUILD_MODE})
                    list(APPEND TBB_IMPORTED_TARGETS TBB::${_tbb_component})
                    set(TBB_${_tbb_component}_FOUND 1)
                endif()
                unset(${_tbb_component_lib_name}_lib CACHE)
                unset(${_tbb_component_lib_name}_dll CACHE)
                unset(${_tbb_component_lib_name}_so CACHE)
                unset(_tbb_component_lib_name)
            endforeach()
        endif()
    endforeach()
    unset(_TBB_BUILD_MODESS)
    unset(_TBB_DEBUG_SUFFIX)
endif()
unset(_tbb_include_dir CACHE)

list(REMOVE_DUPLICATES TBB_IMPORTED_TARGETS)

find_package_handle_standard_args(TBB
                                  REQUIRED_VARS TBB_IMPORTED_TARGETS
                                  HANDLE_COMPONENTS)
