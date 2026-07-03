# Copyright (c) 2020-2023 Intel Corporation
# Copyright (c) 2026 UXL Foundation Contributors
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

find_package(PythonInterp 3.5 REQUIRED)

set(PYTHON_BUILD_WORK_DIR python_build)

configure_file( ${CMAKE_CURRENT_SOURCE_DIR}/setup.py.in  ${CMAKE_CURRENT_SOURCE_DIR}/setup.py)

add_custom_target(
    python_copy
    COMMAND ${CMAKE_COMMAND} -E copy_directory ${CMAKE_CURRENT_SOURCE_DIR}/tbb ${PYTHON_BUILD_WORK_DIR}/tbb
    COMMAND ${CMAKE_COMMAND} -E copy ${CMAKE_CURRENT_SOURCE_DIR}/TBB.py ${PYTHON_BUILD_WORK_DIR}
    COMMAND ${CMAKE_COMMAND} -E copy ${CMAKE_CURRENT_SOURCE_DIR}/setup.py ${PYTHON_BUILD_WORK_DIR}
    COMMAND ${CMAKE_COMMAND} -E copy ${CMAKE_CURRENT_SOURCE_DIR}/pyproject.toml ${PYTHON_BUILD_WORK_DIR}
) 

# Python build requires path to TBB headers
get_target_property(TBB_INCLUDES tbb INTERFACE_INCLUDE_DIRECTORIES)
foreach(dir ${TBB_INCLUDES})
    if (${dir} MATCHES "<BUILD_INTERFACE:" OR TBB_FOUND)
        set(TBB4PY_INCLUDE_STRING "${dir} ${TBB4PY_INCLUDE_STRING}")
    endif()
endforeach()
string(STRIP "${TBB4PY_INCLUDE_STRING}" TBB4PY_INCLUDE_STRING)

add_custom_target(
    python_build
    ALL
    DEPENDS tbb python_copy
    COMMAND
    ${CMAKE_COMMAND} -E env
        TBB_INCLUDEDIRS="${TBB4PY_INCLUDE_STRING}"
        TBB_LIBDIRS=$<TARGET_FILE_DIR:TBB::tbb>
    ${PYTHON_EXECUTABLE} -m pip install
        --no-build-isolation
        --prefix build
        ./${PYTHON_BUILD_WORK_DIR}
    COMMENT "Build and install to work directory the oneTBB Python module"
)

add_test(NAME python_test
         COMMAND ${CMAKE_COMMAND}
                 -DTBB_BINARIES_PATH=$<TARGET_FILE_DIR:TBB::tbb>
                 -DPYTHON_MODULE_BUILD_PATH=${PYTHON_BUILD_WORK_DIR}/build
                 -P ${PROJECT_SOURCE_DIR}/cmake/python/test_launcher.cmake)

install(DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/build/
        DESTINATION .
        COMPONENT tbb4py)

if (UNIX AND NOT APPLE)
    add_subdirectory(rml)
    add_dependencies(python_build irml)
endif()
