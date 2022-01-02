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

# Save current location,
# see for details: https://cmake.org/cmake/help/latest/variable/CMAKE_CURRENT_LIST_DIR.html
set(_tbb_gen_cfg_path ${CMAKE_CURRENT_LIST_DIR})

include(CMakeParseArguments)

function(tbb_generate_config)
    set(options      HANDLE_SUBDIRS)
    set(oneValueArgs INSTALL_DIR
                     SYSTEM_NAME
                     LIB_REL_PATH INC_REL_PATH DLL_REL_PATH
                     VERSION
                     TBB_BINARY_VERSION
                     TBBMALLOC_BINARY_VERSION
                     TBBMALLOC_PROXY_BINARY_VERSION
                     TBBBIND_BINARY_VERSION)

    cmake_parse_arguments(tbb_gen_cfg "${options}" "${oneValueArgs}" "${multiValueArgs}" ${ARGN})

    get_filename_component(config_install_dir ${tbb_gen_cfg_INSTALL_DIR} ABSOLUTE)
    file(MAKE_DIRECTORY ${config_install_dir})

    file(TO_CMAKE_PATH "${tbb_gen_cfg_LIB_REL_PATH}" TBB_LIB_REL_PATH)
    file(TO_CMAKE_PATH "${tbb_gen_cfg_DLL_REL_PATH}" TBB_DLL_REL_PATH)
    file(TO_CMAKE_PATH "${tbb_gen_cfg_INC_REL_PATH}" TBB_INC_REL_PATH)

    set(TBB_VERSION ${tbb_gen_cfg_VERSION})

    set(_tbb_pc_lib_name tbb)
    set(_prefix_for_pc_file "\${pcfiledir}/../../")
    set(_includedir_for_pc_file "\${prefix}/include")

    set(TBB_COMPONENTS_BIN_VERSION "
set(_tbb_bin_version ${tbb_gen_cfg_TBB_BINARY_VERSION})
set(_tbbmalloc_bin_version ${tbb_gen_cfg_TBBMALLOC_BINARY_VERSION})
set(_tbbmalloc_proxy_bin_version ${tbb_gen_cfg_TBBMALLOC_PROXY_BINARY_VERSION})
set(_tbbbind_bin_version ${tbb_gen_cfg_TBBBIND_BINARY_VERSION})
")

    if (tbb_gen_cfg_SYSTEM_NAME STREQUAL "Linux")
        set(TBB_LIB_PREFIX "lib")
        set(TBB_LIB_EXT "so.\${_\${_tbb_component}_bin_version}")
        set(TBB_IMPLIB_RELEASE "")
        set(TBB_IMPLIB_DEBUG "")
        if (tbb_gen_cfg_HANDLE_SUBDIRS)
            set(TBB_HANDLE_SUBDIRS "
if (CMAKE_SIZEOF_VOID_P STREQUAL \"8\")
    set(_tbb_subdir intel64/gcc4.8)
else ()
    set(_tbb_subdir ia32/gcc4.8)
endif()
")

            set(_libdir_for_pc_file "\${prefix}/lib/intel64/gcc4.8")
            configure_file(${_tbb_gen_cfg_path}/../integration/pkg-config/tbb.pc.in ${config_install_dir}/tbb.pc @ONLY)

            set(_libdir_for_pc_file "\${prefix}/lib/ia32/gcc4.8")
            configure_file(${_tbb_gen_cfg_path}/../integration/pkg-config/tbb.pc.in ${config_install_dir}/tbb32.pc @ONLY)
        endif()
    elseif (tbb_gen_cfg_SYSTEM_NAME STREQUAL "Darwin")
        set(TBB_LIB_PREFIX "lib")
        set(TBB_LIB_EXT "\${_\${_tbb_component}_bin_version}.dylib")
        set(TBB_IMPLIB_RELEASE "")
        set(TBB_IMPLIB_DEBUG "")
        set(_libdir_for_pc_file "\${prefix}/lib")
        configure_file(${_tbb_gen_cfg_path}/../integration/pkg-config/tbb.pc.in ${config_install_dir}/tbb.pc @ONLY)
    elseif (tbb_gen_cfg_SYSTEM_NAME STREQUAL "Windows")
        set(TBB_LIB_PREFIX "")
        set(TBB_LIB_EXT "dll")
        set(TBB_COMPILE_DEFINITIONS "
                                  INTERFACE_COMPILE_DEFINITIONS \"__TBB_NO_IMPLICIT_LINKAGE=1\"")

        # .lib files installed to TBB_LIB_REL_PATH (e.g. <prefix>/lib);
        # .dll files installed to TBB_DLL_REL_PATH (e.g. <prefix>/bin);
        # Expand TBB_LIB_REL_PATH here in IMPORTED_IMPLIB property and
        # redefine it with TBB_DLL_REL_PATH value to properly fill IMPORTED_LOCATION property in TBBConfig.cmake.in template.
        set(TBB_IMPLIB_RELEASE "
                                      IMPORTED_IMPLIB_RELEASE \"\${_tbb_root}/${TBB_LIB_REL_PATH}/\${_tbb_subdir}/\${_tbb_component}\${_bin_version}.lib\"")
        set(TBB_IMPLIB_DEBUG "
                                      IMPORTED_IMPLIB_DEBUG \"\${_tbb_root}/${TBB_LIB_REL_PATH}/\${_tbb_subdir}/\${_tbb_component}\${_bin_version}_debug.lib\"")
        set(TBB_LIB_REL_PATH ${TBB_DLL_REL_PATH})

        if (tbb_gen_cfg_HANDLE_SUBDIRS)
            set(TBB_HANDLE_SUBDIRS "
set(_tbb_subdir vc14)
if (WINDOWS_STORE)
    set(_tbb_subdir \${_tbb_subdir}_uwp)
endif()

if (CMAKE_SIZEOF_VOID_P STREQUAL \"8\")
    set(_tbb_subdir intel64/\${_tbb_subdir})
else ()
    set(_tbb_subdir ia32/\${_tbb_subdir})
endif()
")
            set(_tbb_pc_lib_name ${_tbb_pc_lib_name}${TBB_BINARY_VERSION})

            set(_libdir_for_pc_file "\${prefix}/lib/intel64/vc14")
            configure_file(${_tbb_gen_cfg_path}/../integration/pkg-config/tbb.pc.in ${config_install_dir}/tbb.pc @ONLY)

            set(_libdir_for_pc_file "\${prefix}/lib/ia32/vc14")
            configure_file(${_tbb_gen_cfg_path}/../integration/pkg-config/tbb.pc.in ${config_install_dir}/tbb32.pc @ONLY)
        endif()

        set(TBB_HANDLE_BIN_VERSION "
    unset(_bin_version)
    if (_tbb_component STREQUAL tbb)
        set(_bin_version \${_tbb_bin_version})
    endif()
")
    else()
        message(FATAL_ERROR "Unsupported OS name: ${tbb_system_name}")
    endif()

    configure_file(${_tbb_gen_cfg_path}/templates/TBBConfig.cmake.in ${config_install_dir}/TBBConfig.cmake @ONLY)
    configure_file(${_tbb_gen_cfg_path}/templates/TBBConfigVersion.cmake.in ${config_install_dir}/TBBConfigVersion.cmake @ONLY)
endfunction()
