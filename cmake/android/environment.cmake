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

set(ANDROID_DEVICE_TESTING_DIRECTORY "/data/local/tmp/tbb_testing")

find_program(adb_executable adb)
if (NOT adb_executable)
    message(FATAL_ERROR "Could not find adb")
endif()

macro(execute_on_device cmd)
    execute_process(COMMAND ${adb_executable} shell ${cmd} RESULT_VARIABLE CMD_RESULT)
    if (CMD_RESULT)
        message(FATAL_ERROR "Error while on device execution: ${cmd} error_code: ${CMD_RESULT}")
    endif()
endmacro()

macro(transfer_data data_path)
    execute_process(COMMAND ${adb_executable} push --sync ${data_path} ${ANDROID_DEVICE_TESTING_DIRECTORY}
                    RESULT_VARIABLE CMD_RESULT OUTPUT_QUIET)
    if (CMD_RESULT)
        message(FATAL_ERROR "Error while data transferring: ${data_path} error_code: ${CMD_RESULT}")
    endif()
endmacro()
