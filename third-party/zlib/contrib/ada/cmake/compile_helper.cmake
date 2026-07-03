#CMAKE_ARGV0 = /path/to/cmake
#CMAKE_ARGV1 = -P
#CMAKE_ARGV2 = path/to/this/file
#CMAKE_ARGV3 = compiler
#CMAKE_ARGV4 = OBJECT-DIR
#CMAKE_ARGV5 = source-file

if(NOT CMAKE_ARGV3)
    message(FATAL_ERROR "compiler not set")
endif(NOT CMAKE_ARGV3)

if(NOT CMAKE_ARGV4)
    message(FATAL_ERROR "object dir not set")
endif(NOT CMAKE_ARGV4)

if(NOT CMAKE_ARGV5)
    message(FATAL_ERROR "source not set")
endif(NOT CMAKE_ARGV5)

foreach(arg RANGE 6 ${CMAKE_ARGC})
    list(APPEND FLAGS "${CMAKE_ARGV${arg}}")
endforeach(arg RANGE 6 ${CMAKE_ARGC})

execute_process(COMMAND ${CMAKE_ARGV3} compile ${FLAGS} ${CMAKE_ARGV5}
                WORKING_DIRECTORY ${CMAKE_ARGV4}
                RESULT_VARIABLE RESULT
                OUTPUT_VARIABLE dont_care
                ERROR_VARIABLE ERROR)

if(RESULT)
    message(FATAL_ERROR ${RESULT} ${ERROR})
endif(RESULT)
