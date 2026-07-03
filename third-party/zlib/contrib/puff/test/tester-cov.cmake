cmake_minimum_required(VERSION 3.12...3.31)

#CMAKE_ARGV0 = ${CMAKE_COMMAND}
#CMAKE_ARGV1 = -P
#CMAKE_ARGV2 = ${CMAKE_CURRENT_SOURCE_DIR}/tester-cov.cmake
#CMAKE_ARGV3 = "$<TARGET_FILE:puff-test-cov>"
#CMAKE_ARGV4 = "${CMAKE_CURRENT_SOURCE_DIR}"
#CMAKE_ARGV5 = "${CMAKE_CURRENT_BINARY_DIR}"
#CMAKE_ARGV6 = GCOV_EXECUTABLE
#CMAKE_ARGV7 = GCOV_EXECUTABLE

function(puff_cov_test test_string expected_result)
    execute_process(COMMAND ${CMAKE_ARGV0} -E echo_append ${test_string}
                    COMMAND ${CMAKE_ARGV5}
                    COMMAND ${CMAKE_ARGV3}
                    RESULT_VARIABLE RESULT)

    if(NOT RESULT EQUAL expected_result)
        message(FATAL_ERROR "Received Exit-Code: ${RESULT}\n"
                            "Expected Exit-Code: ${expected_result}\n"
                            "Test-String: ${test_string}")
    endif(NOT RESULT EQUAL expected_result)
endfunction(puff_cov_test test_string expected_result)

execute_process(COMMAND ${CMAKE_ARGV3} -w ${CMAKE_ARGV4}/zeros.raw)

puff_cov_test("04" "2")
puff_cov_test("00" "2")
puff_cov_test("00 00 00 00 00" "254")
puff_cov_test("00 01 00 fe ff" "2")

execute_process(COMMAND ${CMAKE_ARGV0} -E echo_append "01 01 00 fe ff 0a"
                COMMAND ${CMAKE_ARGV5}
                COMMAND ${CMAKE_ARGV3})

puff_cov_test("02 7e ff ff" "246")
puff_cov_test("02" "2")
puff_cov_test("04 80 49 92 24 49 92 24 0f b4 ff ff c3 04" "2")
puff_cov_test("04 80 49 92 24 49 92 24 71 ff ff 93 11 00" "249")
puff_cov_test("04 c0 81 08 00 00 00 00 20 7f eb 0b 00 00" "246")

execute_process(COMMAND ${CMAKE_ARGV0} -E echo_append "0b 00 00"
                COMMAND ${CMAKE_ARGV5}
                COMMAND ${CMAKE_ARGV3})

puff_cov_test("1a 07" "246")
puff_cov_test("0c c0 81 00 00 00 00 00 90 ff 6b 04" "245")

execute_process(COMMAND ${CMAKE_ARGV3} -f ${CMAKE_ARGV4}/zeros.raw)

puff_cov_test("fc 00 00" "253")
puff_cov_test("04 00 fe ff" "252")
puff_cov_test("04 00 24 49" "251")
puff_cov_test("04 80 49 92 24 49 92 24 0f b4 ff ff c3 84" "248")
puff_cov_test("04 00 24 e9 ff ff" "250")
puff_cov_test("04 00 24 e9 ff 6d" "247")

execute_process(COMMAND ${CMAKE_ARGV6} ${CMAKE_ARGV7} -n puff.c.gcno)
