cmake_minimum_required(VERSION 3.12...3.31)

#CMAKE_ARGV0 = ${CMAKE_COMMAND}
#CMAKE_ARGV1 = -P
#CMAKE_ARGV2 = ${CMAKE_CURRENT_SOURCE_DIR}/tester.cmake
#CMAKE_ARGV3 = "$<TARGET_FILE:puff-test>"
#CMAKE_ARGV4 = "${CMAKE_CURRENT_SOURCE_DIR}"

execute_process(COMMAND ${CMAKE_ARGV3}
                INPUT_FILE "${CMAKE_ARGV4}/zeros.raw"
                RESULT_VARIABLE RESULT
            COMMAND_ECHO STDERR)

if(RESULT)
    message(FATAL_ERROR "Command exitited with: ${RESULT}")
endif(RESULT)
