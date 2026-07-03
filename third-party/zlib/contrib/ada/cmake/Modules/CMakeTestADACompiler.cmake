include(CMakeTestCompilerCommon)
unset(CMAKE_ADA_COMPILER_WORKS CACHE)

if(NOT CMAKE_ADA_COMPILER_WORKS)
    PrintTestCompilerStatus("ADA" "")
    set(_ADA_TEST_FILE "${CMAKE_BINARY_DIR}/${CMAKE_FILES_DIRECTORY}/CMakeTmp/main.adb")

    file(WRITE ${_ADA_TEST_FILE}
        "with Ada.Text_IO; use Ada.Text_IO;\n"
        "\n"
        "procedure main is\n"
        "begin\n"
        "Put_Line(\"Hello, World!\");\n"
        "end Main;\n")

    try_compile(CMAKE_ADA_COMPILER_WORKS ${CMAKE_BINARY_DIR}
                ${_ADA_TEST_FILE}
                OUTPUT_VARIABLE __CMAKE_ADA_COMPILER_OUTPUT)

    set(CMAKE_ADA_COMPILER_WORKS ${CMAKE_ADA_COMPILER_WORKS})
    unset(CMAKE_ADA_COMPILER_WORKS CACHE)
    set(ADA_TEST_WAS_RUN TRUE)
endif(NOT CMAKE_ADA_COMPILER_WORKS)

if(NOT CMAKE_ADA_COMPILER_WORKS)
    PrintTestCompilerStatus("ADA" " -- broken")

    file(APPEND ${CMAKE_BINARY_DIR}${CMAKE_FILES_DIRECTORY}/CMakeError.log
        "Determining if the Ada compiler works failed with "
        "the following output:\n${__CMAKE_ADA_COMPILER_OUTPUT}\n\n")

    message(FATAL_ERROR "The Ada compiler \"${CMAKE_ADA_COMPILER}\" "
        "is not able to compile a simple test program.\nIt fails "
        "with the following output:\n ${__CMAKE_ADA_COMPILER_OUTPUT}\n\n"
        "CMake will not be able to correctly generate this project.")
else(NOT CMAKE_ADA_COMPILER_WORKS)
    if(ADA_TEST_WAS_RUN)
        PrintTestCompilerStatus("ADA" " -- works")

        file(APPEND ${CMAKE_BINARY_DIR}${CMAKE_FILES_DIRECTORY}/CMakeOutput.log
            "Determining if the Ada compiler works passed with "
            "the following output:\n${__CMAKE_ADA_COMPILER_OUTPUT}\n\n")
    endif(ADA_TEST_WAS_RUN)
endif(NOT CMAKE_ADA_COMPILER_WORKS)

unset(__CMAKE_ADA_COMPILER_OUTPUT)
