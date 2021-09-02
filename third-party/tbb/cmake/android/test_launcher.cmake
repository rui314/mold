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

include(${CMAKE_CURRENT_LIST_DIR}/environment.cmake)

# transfer data to device
execute_on_device("mkdir -m 755 -p ${ANDROID_DEVICE_TESTING_DIRECTORY}")

file (GLOB_RECURSE BINARIES_LIST "${BINARIES_PATH}/*.so*" "${BINARIES_PATH}/${TEST_NAME}")
foreach(BINARY_FILE ${BINARIES_LIST})
    transfer_data(${BINARY_FILE})
endforeach()

# execute binary
execute_on_device("chmod -R 755 ${ANDROID_DEVICE_TESTING_DIRECTORY}")
execute_on_device("LD_LIBRARY_PATH=${ANDROID_DEVICE_TESTING_DIRECTORY} ${ANDROID_DEVICE_TESTING_DIRECTORY}/${TEST_NAME}")
