#CMAKE_ARGV0 = /path/to/cmake
#CMAKE_ARGV1 = -P
#CMAKE_ARGV2 = path/to/this/file
#CMAKE_ARGV3 = binder
#CMAKE_ARGV4 = ali

if(NOT CMAKE_ARGV3)
    message(FATAL_ERROR "binder not set")
endif(NOT CMAKE_ARGV3)

string(REPLACE ".o" ".ali" ALI ${CMAKE_ARGV4})

set (REACHED_FLAGS FALSE)
#iterate over additional objects, only the main one is needed
foreach(arg RANGE 5 ${CMAKE_ARGC})
    if(CMAKE_ARGV${arg} STREQUAL FLAGS)
        set(REACHED_FLAGS TRUE)
        continue()
    endif(CMAKE_ARGV${arg} STREQUAL FLAGS)

    string(SUBSTRING "${CMAKE_ARGV${arg}}" 0 2 start)

    if(start STREQUAL "-O")
        continue()
    endif(start STREQUAL "-O")

    if(REACHED_FLAGS)
        list(APPEND FLAGS ${CMAKE_ARGV${arg}})
    endif(REACHED_FLAGS)
endforeach(arg RANGE 5 CMAKE_ARGC)

#first see if there is a main function
execute_process(COMMAND ${CMAKE_ARGV3} bind ${ALI} ${FLAGS}
                RESULT_VARIABLE MAIN_RESULT
                OUTPUT_VARIABLE dont_care
                ERROR_VARIABLE ERROR)

if(MAIN_RESULT)
    execute_process(COMMAND ${CMAKE_ARGV3} bind -n ${ALI} ${FLAGS}
                    RESULT_VARIABLE RESULT
                    OUTPUT_VARIABLE dont_care
                    ERROR_VARIABLE ERROR)
endif(MAIN_RESULT)

if(RESULT)
    message(FATAL_ERROR ${RESULT} ${ERROR})
endif(RESULT)
