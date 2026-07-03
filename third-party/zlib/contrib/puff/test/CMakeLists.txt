# if we are built from with zlib, use this path's)
if(DEFINED ZLIB_BUILD_PUFF)
    set(WORK_DIR ${zlib_BINARY_DIR})
    set(inst_setup zlib_install)
else(DEFINED ZLIB_BUILD_PUFF)
    set(WORK_DIR ${puff_BINARY_DIR})
    set(inst_setup zlib_puff_install)
    set(ZLIB_ARG "-DZLIB_DIR=${ZLIB_DIR}")

    add_test(
        NAME zlib_puff_install
        COMMAND ${CMAKE_COMMAND} --install ${puff_BINARY_DIR} --prefix
                ${CMAKE_CURRENT_BINARY_DIR}/test_install --config $<CONFIG>
        WORKING_DIRECTORY ${puff_BINARY_DIR})

    set_tests_properties(zlib_puff_install
        PROPERTIES
            FIXTURES_SETUP zlib_puff_install)
endif(DEFINED ZLIB_BUILD_PUFF)

if(ZLIB_PUFF_BUILD_SHARED AND NOT WIN32)
    add_executable(zlib_puff_test
        ${puff_SOURCE_DIR}/pufftest.c
        ${puff_SOURCE_DIR}/puff.h)

    target_link_libraries(zlib_puff_test
        PRIVATE zlib_puff_puff)

    add_test(NAME zlib_puff_test
        COMMAND ${CMAKE_COMMAND} -P
                ${CMAKE_CURRENT_SOURCE_DIR}/tester.cmake
                "$<TARGET_FILE:zlib_puff_test>"
                "${puff_SOURCE_DIR}")

    if(GCOV_EXECUTABLE)
        add_executable(zlib_puff_test-coverage
            ${puff_SOURCE_DIR}/pufftest.c
            ${puff_SOURCE_DIR}/puff.c
            ${puff_SOURCE_DIR}/puff.h)

        target_compile_options(zlib_puff_test-coverage PRIVATE -coverage)

        target_link_options(zlib_puff_test-coverage PRIVATE -coverage)

        add_test(NAME zlib_puff_test-coverage
            COMMAND ${CMAKE_COMMAND} -P
                    ${CMAKE_CURRENT_SOURCE_DIR}/tester-cov.cmake
                    "$<TARGET_FILE:zlib_puff_test-coverage>"
                    "${puff_SOURCE_DIR}"
                    "$<TARGET_FILE:zlib_puff_bin-writer>"
                    ${GCOV_EXECUTABLE}
                    ${llvm_option})
    endif(GCOV_EXECUTABLE)
endif(ZLIB_PUFF_BUILD_SHARED AND NOT WIN32)

if(ZLIB_PUFF_BUILD_STATIC AND NOT WIN32)
    add_executable(zlib_puff_testStatic
        ${puff_SOURCE_DIR}/pufftest.c
        ${puff_SOURCE_DIR}/puff.h)

    target_link_libraries(zlib_puff_testStatic
        PRIVATE zlib_puff_puffStatic)
    add_test(NAME zlib_puff_testStatic
        COMMAND ${CMAKE_COMMAND} -P
                ${CMAKE_CURRENT_SOURCE_DIR}/tester.cmake
                "$<TARGET_FILE:zlib_puff_testStatic>"
                "${puff_SOURCE_DIR}")

    if(GCOV_EXECUTABLE)
        add_executable(zlib_puff_testStatic-coverage
            ${puff_SOURCE_DIR}/pufftest.c
            ${puff_SOURCE_DIR}/puff.c
            ${puff_SOURCE_DIR}/puff.h)

        target_compile_options(zlib_puff_testStatic-coverage
            PRIVATE -coverage)

        target_link_options(zlib_puff_testStatic-coverage
            PRIVATE -coverage)

        add_test(NAME zlib_puff_testStatic-coverage
            COMMAND ${CMAKE_COMMAND} -P
                ${CMAKE_CURRENT_SOURCE_DIR}/tester-cov.cmake
                "$<TARGET_FILE:zlib_puff_testStatic-coverage>"
                "${puff_SOURCE_DIR}"
                "$<TARGET_FILE:zlib_puff_bin-writer>"
                ${GCOV_EXECUTABLE}
                ${llvm_option})
    endif(GCOV_EXECUTABLE)
endif(ZLIB_PUFF_BUILD_STATIC AND NOT WIN32)

file(MAKE_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/findpackage_test)
file(MAKE_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/add_subdirectory_test)
file(MAKE_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/add_subdirectory_exclude_test)

configure_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/find_package_test.cmake.in
    ${CMAKE_CURRENT_BINARY_DIR}/findpackage_test/CMakeLists.txt @ONLY)

configure_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/add_subdirectory_test.cmake.in
    ${CMAKE_CURRENT_BINARY_DIR}/add_subdirectory_test/CMakeLists.txt @ONLY)

configure_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/add_subdirectory_exclude_test.cmake.in
    ${CMAKE_CURRENT_BINARY_DIR}/add_subdirectory_exclude_test/CMakeLists.txt
    @ONLY)

configure_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/find_package_no_components_test.cmake.in
    ${CMAKE_CURRENT_BINARY_DIR}/findpackage_no_components_test/CMakeLists.txt
    @ONLY)

configure_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/find_package_wrong_components_test.cmake.in
    ${CMAKE_CURRENT_BINARY_DIR}/findpackage_wrong_components_test/CMakeLists.txt
    @ONLY)

# CMAKE_GENERATOR_PLATFORM doesn't work in the if
set(GENERATOR ${CMAKE_GENERATOR_PLATFORM})

if(GENERATOR)
    set(PLATFORM "-A ${GENERATOR}")
endif(GENERATOR)

#
# findpackage_test
#
add_test(
    NAME zlib_puff_find_package_configure
    COMMAND
        ${CMAKE_COMMAND}
        ${PLATFORM}
        -B${CMAKE_CURRENT_BINARY_DIR}/findpackage_test_build
        -DCMAKE_BUILD_TYPE=$<CONFIG>
        -DCMAKE_C_COMPILER=${CMAKE_C_COMPILER}
        -DCMAKE_C_FLAGS=${CMAKE_C_FLAGS}
        -DCMAKE_INSTALL_PREFIX=${WORK_DIR}/test/test_install
        ${ZLIB_ARG}
        --fresh
        -G "${CMAKE_GENERATOR}"
        -S${CMAKE_CURRENT_BINARY_DIR}/findpackage_test)

add_test(
    NAME zlib_puff_find_package_build
    COMMAND ${CMAKE_COMMAND} --build . --config $<CONFIG>
    WORKING_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/findpackage_test_build)

set_tests_properties(zlib_puff_find_package_configure
    PROPERTIES
        FIXTURES_REQUIRED ${inst_setup}
        FIXTURES_SETUP puff_fp_config)

set_tests_properties(zlib_puff_find_package_build
    PROPERTIES
        FIXTURES_REQUIRED puff_fp_config)

#
# add_subdirectory_test
#
add_test(
    NAME zlib_puff_add_subdirectory_configure
    COMMAND
        ${CMAKE_COMMAND} ${PLATFORM}
        -B${CMAKE_CURRENT_BINARY_DIR}/add_subdirectory_test_build
        -DCMAKE_BUILD_TYPE=$<CONFIG>
        -DCMAKE_C_COMPILER=${CMAKE_C_COMPILER}
        -DCMAKE_C_FLAGS=${CMAKE_C_FLAGS}
        -DCMAKE_INSTALL_PREFIX=${WORK_DIR}/test/test_install ${ZLIB_ARG}
        --fresh
        -G "${CMAKE_GENERATOR}"
        -S${CMAKE_CURRENT_BINARY_DIR}/add_subdirectory_test)

add_test(
    NAME zlib_puff_add_subdirectory_build
    COMMAND ${CMAKE_COMMAND} --build . --config $<CONFIG>
    WORKING_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/add_subdirectory_test_build)

set_tests_properties(zlib_puff_add_subdirectory_configure
    PROPERTIES
        FIXTURES_REQUIRED ${inst_setup}
        FIXTURES_SETUP puiff_as_config)

set_tests_properties(zlib_puff_add_subdirectory_build
    PROPERTIES
        FIXTURES_REQUIRED puff_as_config)

#
# add_subdirectory_exclude_test
#
add_test(
    NAME zlib_puff_add_subdirectory_exclude_configure
    COMMAND
        ${CMAKE_COMMAND} ${PLATFORM}
        -B${CMAKE_CURRENT_BINARY_DIR}/add_subdirectory_exclude_test_build
        -DCMAKE_BUILD_TYPE=$<CONFIG> -DCMAKE_C_COMPILER=${CMAKE_C_COMPILER}
        -DCMAKE_C_FLAGS=${CMAKE_C_FLAGS}
        -DCMAKE_INSTALL_PREFIX=${WORK_DIR}/test/test_install ${ZLIB_ARG}
        --fresh
        -G "${CMAKE_GENERATOR}"
        -S${CMAKE_CURRENT_BINARY_DIR}/add_subdirectory_exclude_test)

add_test(
    NAME zlib_puff_add_subdirectory_exclude_build
    COMMAND ${CMAKE_COMMAND} --build . --config $<CONFIG>
    WORKING_DIRECTORY
        ${CMAKE_CURRENT_BINARY_DIR}/add_subdirectory_exclude_test_build)

set_tests_properties(zlib_puff_add_subdirectory_exclude_configure
    PROPERTIES
        FIXTURES_REQUIRED ${inst_setup}
        FIXTURES_SETUP puff_asx_config)

set_tests_properties(zlib_puff_add_subdirectory_exclude_build
    PROPERTIES
        FIXTURES_REQUIRED puff_asx_config)

#
# findpackage_no_components_test
#
add_test(
    NAME zlib_puff_find_package_no_components_configure
    COMMAND
        ${CMAKE_COMMAND} ${PLATFORM}
        -B${CMAKE_CURRENT_BINARY_DIR}/findpackage_no_components_test_build
        -DCMAKE_BUILD_TYPE=$<CONFIG> -DCMAKE_C_COMPILER=${CMAKE_C_COMPILER}
        -DCMAKE_C_FLAGS=${CMAKE_C_FLAGS}
        -DCMAKE_INSTALL_PREFIX=${WORK_DIR}/test/test_install ${ZLIB_ARG}
        --fresh
        -G "${CMAKE_GENERATOR}"
        -S${CMAKE_CURRENT_BINARY_DIR}/findpackage_no_components_test)

set_tests_properties(
    zlib_puff_find_package_no_components_configure
        PROPERTIES
            FIXTURES_REQUIRED ${inst_setup})

if(NOT ZLIB_PUFF_BUILD_SHARED OR NOT ZLIB_PUFF_BUILD_STATIC)
    set_tests_properties(zlib_puff_find_package_no_components_configure
        PROPERTIES
            WILL_FAIL TRUE)
endif(NOT ZLIB_PUFF_BUILD_SHARED OR NOT ZLIB_PUFF_BUILD_STATIC)

#
# findpackage_no_components_test
#
add_test(
    NAME zlib_puff_find_package_wrong_components_configure
    COMMAND
        ${CMAKE_COMMAND} ${PLATFORM}
        -B${CMAKE_CURRENT_BINARY_DIR}/findpackage_wrong_components_test_build
        -DCMAKE_BUILD_TYPE=$<CONFIG> -DCMAKE_C_COMPILER=${CMAKE_C_COMPILER}
        -DCMAKE_C_FLAGS=${CMAKE_C_FLAGS}
        -DCMAKE_INSTALL_PREFIX=${WORK_DIR}/test/test_install ${ZLIB_ARG}
        --fresh
        -G "${CMAKE_GENERATOR}"
        -S${CMAKE_CURRENT_BINARY_DIR}/findpackage_wrong_components_test)

set_tests_properties(zlib_puff_find_package_wrong_components_configure
    PROPERTIES
        FIXTURES_REQUIRED ${inst_setup}
        WILL_FAIL TRUE)
