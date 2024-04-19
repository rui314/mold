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

# Save current location,
# see for details: https://cmake.org/cmake/help/latest/variable/CMAKE_CURRENT_LIST_DIR.html
set(_tbb_gen_cfg_path ${CMAKE_CURRENT_LIST_DIR})

include(CMakeParseArguments)

function(tbb_generate_config)
    set(options      HANDLE_SUBDIRS)
    set(oneValueArgs INSTALL_DIR
                     SYSTEM_NAME
                     LIB_REL_PATH INC_REL_PATH
                     VERSION
                     TBB_BINARY_VERSION
                     TBBMALLOC_BINARY_VERSION
                     TBBMALLOC_PROXY_BINARY_VERSION
                     TBBBIND_BINARY_VERSION)

    cmake_parse_arguments(tbb_gen_cfg "${options}" "${oneValueArgs}" "${multiValueArgs}" ${ARGN})

    get_filename_component(config_install_dir ${tbb_gen_cfg_INSTALL_DIR} ABSOLUTE)
    file(MAKE_DIRECTORY ${config_install_dir})

    file(TO_CMAKE_PATH "${tbb_gen_cfg_LIB_REL_PATH}" TBB_LIB_REL_PATH)
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

        set (TBB_HANDLE_IMPLIB "
            set (_tbb_release_dll \${_tbb_release_lib})
            set (_tbb_debug_dll \${_tbb_debug_lib})
")
        if (tbb_gen_cfg_HANDLE_SUBDIRS)
            set(TBB_HANDLE_SUBDIRS "set(_tbb_subdir gcc4.8)")

            set(_libdir_for_pc_file "\${prefix}/lib/intel64/gcc4.8")
            set(_tbb_pc_extra_libdir "-L\${prefix}/lib")
            configure_file(${_tbb_gen_cfg_path}/../integration/pkg-config/tbb.pc.in ${config_install_dir}/tbb.pc @ONLY)

            set(_libdir_for_pc_file "\${prefix}/lib/ia32/gcc4.8")
            set(_tbb_pc_extra_libdir "-L\${prefix}/lib32")
            configure_file(${_tbb_gen_cfg_path}/../integration/pkg-config/tbb.pc.in ${config_install_dir}/tbb32.pc @ONLY)
        endif()
    elseif (tbb_gen_cfg_SYSTEM_NAME STREQUAL "Darwin")
        set(TBB_LIB_PREFIX "lib")
        set(TBB_LIB_EXT "\${_\${_tbb_component}_bin_version}.dylib")

        set (TBB_HANDLE_IMPLIB "
            set (_tbb_release_dll \${_tbb_release_lib})
            set (_tbb_debug_dll \${_tbb_debug_lib})
")
        set(_libdir_for_pc_file "\${prefix}/lib")
        configure_file(${_tbb_gen_cfg_path}/../integration/pkg-config/tbb.pc.in ${config_install_dir}/tbb.pc @ONLY)
    elseif (tbb_gen_cfg_SYSTEM_NAME STREQUAL "Windows")
        set(TBB_LIB_PREFIX "")
        set(TBB_LIB_EXT "lib")
        set(TBB_COMPILE_DEFINITIONS "
                                  INTERFACE_COMPILE_DEFINITIONS \"__TBB_NO_IMPLICIT_LINKAGE=1\"")
        
        # .lib - installed to TBB_LIB_REL_PATH (e.g. <prefix>/lib) and are passed as IMPORTED_IMPLIB_<CONFIG> property to target
        # .dll - installed to <prefix>/bin or <prefix>/redist and are passed as IMPORTED_LOCATION_<CONFIG> property to target
        set (TBB_HANDLE_IMPLIB "
            find_file(_tbb_release_dll
                NAMES \${_tbb_component}\${_bin_version}.dll
                PATHS \${_tbb_root}
                PATH_SUFFIXES \"redist/\${_tbb_intel_arch}/\${_tbb_subdir}\" \"bin\${_tbb_arch_suffix}/\${_tbb_subdir}\" \"bin\${_tbb_arch_suffix}/\" \"bin\"
            )

            if (EXISTS \"\${_tbb_debug_lib}\")
                find_file(_tbb_debug_dll
                    NAMES \${_tbb_component}\${_bin_version}_debug.dll
                    PATHS \${_tbb_root}
                    PATH_SUFFIXES \"redist/\${_tbb_intel_arch}/\${_tbb_subdir}\" \"bin\${_tbb_arch_suffix}/\${_tbb_subdir}\" \"bin\${_tbb_arch_suffix}/\" \"bin\"
                )
            endif()
")
            set(TBB_IMPLIB_RELEASE "
                                        IMPORTED_IMPLIB_RELEASE \"\${_tbb_release_lib}\"")
            set(TBB_IMPLIB_DEBUG "
                                        IMPORTED_IMPLIB_DEBUG \"\${_tbb_debug_lib}\"")

        if (tbb_gen_cfg_HANDLE_SUBDIRS)
            set(TBB_HANDLE_SUBDIRS "
set(_tbb_subdir vc14)
if (WINDOWS_STORE)
    set(_tbb_subdir \${_tbb_subdir}_uwp)
endif()
")
            set(_tbb_pc_lib_name ${_tbb_pc_lib_name}${TBB_BINARY_VERSION})

            set(_libdir_for_pc_file "\${prefix}/lib/intel64/vc14")
            set(_tbb_pc_extra_libdir "-L\${prefix}/lib")
            configure_file(${_tbb_gen_cfg_path}/../integration/pkg-config/tbb.pc.in ${config_install_dir}/tbb.pc @ONLY)

            set(_libdir_for_pc_file "\${prefix}/lib/ia32/vc14")
            set(_tbb_pc_extra_libdir "-L\${prefix}/lib32")
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
