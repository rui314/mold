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

find_package(PythonInterp 3.5 REQUIRED)

file(GLOB_RECURSE MODULES_LIST "${PYTHON_MODULE_BUILD_PATH}/*TBB.py*" )
list(LENGTH MODULES_LIST MODULES_COUNT)

if (MODULES_COUNT EQUAL 0)
    message(FATAL_ERROR "Cannot find oneTBB Python module")
elseif (MODULES_COUNT GREATER 1)
    message(WARNING "Found more than oneTBB Python modules, the only first found module will be tested")
endif()

list(GET MODULES_LIST 0 PYTHON_MODULE)
get_filename_component(PYTHON_MODULE_PATH ${PYTHON_MODULE} DIRECTORY)

execute_process(
    COMMAND
        ${CMAKE_COMMAND} -E env LD_LIBRARY_PATH=${TBB_BINARIES_PATH}
        ${PYTHON_EXECUTABLE} -m tbb test
    WORKING_DIRECTORY ${PYTHON_MODULE_PATH}
    RESULT_VARIABLE CMD_RESULT
)
if (CMD_RESULT)
    message(FATAL_ERROR "Error while test execution: ${cmd} error_code: ${CMD_RESULT}")
endif()
