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

macro(tbb_remove_compile_flag flag)
    get_property(_tbb_compile_options DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR} PROPERTY COMPILE_OPTIONS)
    list(REMOVE_ITEM _tbb_compile_options ${flag})
    set_property(DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR} PROPERTY COMPILE_OPTIONS ${_tbb_compile_options})
    unset(_tbb_compile_options)
    if (CMAKE_CXX_FLAGS)
        string(REGEX REPLACE "(^|[ \t\r\n]+)${flag}($|[ \t\r\n]+)" " " CMAKE_CXX_FLAGS ${CMAKE_CXX_FLAGS})
    endif()
endmacro()

macro(tbb_install_target target)
    install(TARGETS ${target}
        EXPORT TBBTargets
        LIBRARY
            DESTINATION ${CMAKE_INSTALL_LIBDIR}
            NAMELINK_SKIP
            COMPONENT runtime
        RUNTIME
            DESTINATION ${CMAKE_INSTALL_BINDIR}
            COMPONENT runtime
        ARCHIVE
            DESTINATION ${CMAKE_INSTALL_LIBDIR}
            COMPONENT devel)

    if (BUILD_SHARED_LIBS)
        install(TARGETS ${target}
            LIBRARY
                DESTINATION ${CMAKE_INSTALL_LIBDIR}
                NAMELINK_ONLY
                COMPONENT devel)
    endif()
    if (MSVC AND BUILD_SHARED_LIBS)
        install(FILES $<TARGET_PDB_FILE:${target}>
            DESTINATION ${CMAKE_INSTALL_BINDIR}
            COMPONENT devel
            OPTIONAL)
    endif()
endmacro()

macro(tbb_handle_ipo target)
    if (TBB_IPO_PROPERTY)
        set_target_properties(${target} PROPERTIES 
            INTERPROCEDURAL_OPTIMIZATION TRUE
            INTERPROCEDURAL_OPTIMIZATION_DEBUG FALSE
        )
    elseif (TBB_IPO_FLAGS)
        target_compile_options(${target} PRIVATE ${TBB_IPO_COMPILE_FLAGS})
        if (COMMAND target_link_options)
            target_link_options(${target} PRIVATE ${TBB_IPO_LINK_FLAGS})
        else()
            target_link_libraries(${target} PRIVATE ${TBB_IPO_LINK_FLAGS})
        endif()
    endif()
endmacro()
