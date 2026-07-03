cmake_minimum_required(VERSION 3.12...3.31)

#CMAKE_ARGV0 = ${CMAKE_COMMAND}
#CMAKE_ARGV1 = -P
#CMAKE_ARGV2 = ${CMAKE_CURRENT_SOURCE_DIR}/tester.cmake
#CMAKE_ARGV3 = "$<TARGET_FILE:blast-test>"
#CMAKE_ARGV4 = "${CMAKE_CURRENT_SOURCE_DIR}"
#CMAKE_ARGV5 = "${CMAKE_CURRENT_BINARY_DIR}")

execute_process(COMMAND ${CMAKE_ARGV3}
                INPUT_FILE "${CMAKE_ARGV4}/test.pk"
                OUTPUT_FILE "${CMAKE_ARGV5}/output.txt"
                RESULT_VARIABLE RESULT)

if(RESULT)
    message(FATAL_ERROR "Command exitited with: ${RESULT}")
endif(RESULT)

execute_process(COMMAND ${CMAKE_ARGV0} -E compare_files
                        "${CMAKE_ARGV4}/test.txt"
                        "${CMAKE_ARGV5}/output.txt"
                RESULT_VARIABLE RESULT)

file(REMOVE "${CMAKE_ARGV5}/output.txt")

if(RESULT)
    message(FATAL_ERROR "Files differ")
endif(RESULT)
