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

cmake_minimum_required(VERSION 3.5)

set(EXAMPLES_UI_MODE "con" CACHE STRING "EXAMPLES_UI_MODE")

if (WIN32)
    set_property(CACHE EXAMPLES_UI_MODE PROPERTY STRINGS "con" "gdi" "d2d")
elseif (APPLE)
    set_property(CACHE EXAMPLES_UI_MODE PROPERTY STRINGS "con" "mac")
else()
    set_property(CACHE EXAMPLES_UI_MODE PROPERTY STRINGS "con" "x")
endif()

get_property(_available_ui_modes CACHE EXAMPLES_UI_MODE PROPERTY STRINGS)

list(FIND _available_ui_modes ${EXAMPLES_UI_MODE} _find_index)
if  (NOT _find_index EQUAL -1)
    add_library(UI_LIB_${PROJECT_NAME} STATIC ${CMAKE_CURRENT_LIST_DIR}/${EXAMPLES_UI_MODE}video.cpp)

    if (EXAMPLES_UI_MODE STREQUAL "mac")
        enable_language(C)
        add_library(UI_LIB_OBJECT_${PROJECT_NAME} OBJECT  
            ${CMAKE_CURRENT_LIST_DIR}/xcode/tbbExample/main.m 
            ${CMAKE_CURRENT_LIST_DIR}/xcode/tbbExample/OpenGLView.m 
            ${CMAKE_CURRENT_LIST_DIR}/xcode/tbbExample/tbbAppDelegate.m
        )
        target_sources(UI_LIB_${PROJECT_NAME} PUBLIC $<TARGET_OBJECTS:UI_LIB_OBJECT_${PROJECT_NAME}>)
    endif()
else()
    string(REPLACE ";" ", " _available_ui_modes "${_available_ui_modes}")
    message(FATAL_ERROR
        "Selected UI mode (${EXAMPLES_UI_MODE}) is not supported on ${CMAKE_SYSTEM_NAME}. 
        Supported UI modes: ${_available_ui_modes}"
    )
endif()

macro(set_gdi_ui_project_settings)
    find_program(RC rc REQUIRED)
    add_custom_command(TARGET UI_LIB_${PROJECT_NAME} PRE_BUILD COMMAND
        ${RC} /r -fo ${PROJECT_BINARY_DIR}/${PROJECT_NAME}.res ${PROJECT_SOURCE_DIR}/gui/${PROJECT_NAME}.rc
    )

    set_target_properties(${PROJECT_NAME} PROPERTIES WIN32_EXECUTABLE TRUE)
    if (COMMAND target_link_options)
        target_link_options(UI_LIB_${PROJECT_NAME} PUBLIC ${PROJECT_BINARY_DIR}/${PROJECT_NAME}.res)
    else()
        set_target_properties(${PROJECT_NAME} PROPERTIES LINK_FLAGS ${PROJECT_BINARY_DIR}/${PROJECT_NAME}.res)
    endif()
endmacro()

macro(set_d2d_ui_project_settings)
    set_gdi_ui_project_settings()

    if (IS_DIRECTORY $ENV{DXSDK_DIR})
        target_include_directories(UI_LIB_${PROJECT_NAME} PUBLIC $ENV{DXSDK_DIR}\\include)
        target_link_directories(UI_LIB_${PROJECT_NAME} PUBLIC $ENV{DXSDK_DIR}\\lib\\x64)
    else()
        message(FATAL_ERROR "Cannot find the DirectX library (required by the 'd2d' UI mode)")
    endif()
endmacro()

macro(set_x_ui_project_settings)
    find_package(X11 REQUIRED)
    target_link_libraries(UI_LIB_${PROJECT_NAME} PUBLIC  ${X11_LIBRARIES})
    target_include_directories(UI_LIB_${PROJECT_NAME} PUBLIC ${X11_INCLUDE_DIR})

    find_library(LIB_Xext Xext PATHS ENV LIBRARY_PATH ENV LD_LIBRARY_PATH ENV DYLD_LIBRARY_PATH)
    if (LIB_Xext)
        target_link_libraries(UI_LIB_${PROJECT_NAME} PUBLIC  ${LIB_Xext})
    else()
        target_compile_definitions(UI_LIB_${PROJECT_NAME} PUBLIC -DX_NOSHMEM)
    endif()
endmacro()

macro(set_mac_ui_project_settings)
    find_package(OpenGL REQUIRED)

    find_library(FOUNDATION_LIBRARY Foundation PATHS ENV LIBRARY_PATH ENV LD_LIBRARY_PATH ENV DYLD_LIBRARY_PATH)
    if (NOT FOUNDATION_LIBRARY)
        message(FATAL_ERROR "Cannot find the Foundation library (required by the 'mac' UI mode)")
    endif()

    find_library(COCOA_LIBRARY Cocoa PATHS ENV LIBRARY_PATH ENV LD_LIBRARY_PATH ENV DYLD_LIBRARY_PATH)
    if (NOT COCOA_LIBRARY)
        message(FATAL_ERROR "Cannot find the Cocoa library (required by the 'mac' UI mode)")
    endif()

    target_link_libraries(UI_LIB_${PROJECT_NAME} PUBLIC ${OPENGL_LIBRARIES} ${FOUNDATION_LIBRARY} ${COCOA_LIBRARY})
    target_include_directories(UI_LIB_${PROJECT_NAME} PUBLIC ${OPENGL_INCLUDE_DIR})

    file(MAKE_DIRECTORY ${CMAKE_RUNTIME_OUTPUT_DIRECTORY}/MacUI/${PROJECT_NAME}.app/Contents/Resources)
    file(MAKE_DIRECTORY ${CMAKE_RUNTIME_OUTPUT_DIRECTORY}/MacUI/${PROJECT_NAME}.app/Contents/MacOS)

    file(COPY ${CMAKE_CURRENT_SOURCE_DIR}/../../common/gui/xcode/tbbExample/PkgInfo DESTINATION ${CMAKE_RUNTIME_OUTPUT_DIRECTORY}/MacUI/${PROJECT_NAME}.app/Contents)
    file(COPY ${CMAKE_CURRENT_SOURCE_DIR}/../../common/gui/xcode/tbbExample/en.lproj DESTINATION ${CMAKE_RUNTIME_OUTPUT_DIRECTORY}/MacUI/${PROJECT_NAME}.app/Contents/Resources)
    configure_file(${CMAKE_CURRENT_SOURCE_DIR}/../../common/gui/xcode/tbbExample/tbbExample-Info.plist ${CMAKE_RUNTIME_OUTPUT_DIRECTORY}/MacUI/${PROJECT_NAME}.app/Contents/Info.plist COPYONLY)

    set_target_properties(${PROJECT_NAME} PROPERTIES PREFIX "MacUI/${PROJECT_NAME}.app/Contents/MacOS/")
endmacro()

if  (NOT _find_index EQUAL -1)
    if ("${EXAMPLES_UI_MODE}" STREQUAL "gdi")
        set_gdi_ui_project_settings()
    elseif ("${EXAMPLES_UI_MODE}" STREQUAL "d2d")
        set_d2d_ui_project_settings()
    elseif ("${EXAMPLES_UI_MODE}" STREQUAL "mac")
        set_mac_ui_project_settings()
    elseif ("${EXAMPLES_UI_MODE}" STREQUAL "x")
        set_x_ui_project_settings()
    endif()
    target_compile_options(UI_LIB_${PROJECT_NAME} PRIVATE ${TBB_CXX_STD_FLAG})
endif()

unset(_available_ui_modes)
unset(_find_index)
