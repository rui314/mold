cmake_minimum_required(VERSION 3.12...3.31)

project(
    testzlib
    VERSION 1.0.0
    LANGUAGES C
    DESCRIPTION "A little program to test zlib"
    HOMEPAGE_URL "https://www.zlib.net")

option(ZLIB_TESTZLIB_BUILD_SHARED "Enable building testzlib" ON)
option(ZLIB_TESTZLIB_BUILD_STATIC "Enable building static linked testzlib" ON)
option(ZLIB_TESTZLIB_INSTALL "Enable installation of testzlib" ON)

if(ZLIB_TESTZLIB_BUILD_SHARED)
    add_executable(zlib_testzlib testzlib.c)
    target_link_libraries(zlib_testzlib PRIVATE ZLIB::ZLIB)

    set_target_properties(zlib_testzlib
        PROPERTIES
            OUTPUT_NAME testzlib)

    if(ZLIB_TESTZLIB_INSTALL)
        install(
            TARGETS zlib_testzlib
            COMPONENT Runtime
            RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}")
    endif(ZLIB_TESTZLIB_INSTALL)
endif(ZLIB_TESTZLIB_BUILD_SHARED)

if(ZLIB_TESTZLIB_BUILD_STATIC)
    add_executable(zlib_testzlibStatic testzlib.c)
    target_link_libraries(zlib_testzlibStatic PRIVATE ZLIB::ZLIBSTATIC)

    set_target_properties(zlib_testzlibStatic
        PROPERTIES
            OUTPUT_NAME testzlibStatic)

    if(ZLIB_TESTZLIB_INSTALL)
        install(
            TARGETS zlib_testzlibStatic
            COMPONENT Runtime
            RUNTIME DESTINATION "${CMAKE_INSTALL_BINDIR}")
    endif(ZLIB_TESTZLIB_INSTALL)
endif(ZLIB_TESTZLIB_BUILD_STATIC)
