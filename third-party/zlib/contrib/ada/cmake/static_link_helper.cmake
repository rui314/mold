#CMAKE_ARGV0 = /path/to/cmake
#CMAKE_ARGV1 = -P
#CMAKE_ARGV2 = path/to/this/file
#CMAKE_ARGV3 = path/to/ar
#CMAKE_ARGV4 = output-name
#CMAKE_ARGV5...CMAKE_AGVN = OBJECTS

if(NOT CMAKE_ARGV3)
    message(FATAL_ERROR "linker not set")
endif(NOT CMAKE_ARGV3)

foreach(arg RANGE 5 ${CMAKE_ARGC})
    if(NOT CMAKE_ARGC EQUAL arg)
        list(APPEND OBJECT_FILES "${CMAKE_ARGV${arg}}")
    endif(NOT CMAKE_ARGC EQUAL arg)
endforeach(arg RANGE 6 ${CMAKE_ARGC})

execute_process(COMMAND ${CMAKE_ARGV3} rcs ${CMAKE_ARGV4} ${OBJECT_FILES}
                RESULT_VARIABLE RESULT
                OUTPUT_VARIABLE dont_care
                ERROR_VARIABLE ERROR)

if(RESULT)
    message(FATAL_ERROR ${RESULT} ${ERROR})
endif(RESULT)
