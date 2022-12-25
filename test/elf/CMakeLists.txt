option(MOLD_ENABLE_QEMU_TESTS "Enable tests on non-native targets" OFF)
option(MOLD_ENABLE_QEMU_TESTS_RV32 "Enable tests for RV32" OFF)

function(add_target TRIPLE)
  set(HOST ${CMAKE_HOST_SYSTEM_PROCESSOR})
  if(${HOST} MATCHES "arm.*")
    set(HOST arm)
  endif()

  if(${TRIPLE} MATCHES "${HOST}-.*")
    set(IS_NATIVE 1)
  endif()

  if(${TRIPLE} MATCHES "([^-]+)-.")
    set(MACHINE ${CMAKE_MATCH_1})
  endif()

  if(IS_NATIVE OR MOLD_ENABLE_QEMU_TESTS)
    file(GLOB ALL_TESTS RELATIVE ${CMAKE_CURRENT_LIST_DIR} CONFIGURE_DEPENDS
      "*.sh")

    list(FILTER ALL_TESTS EXCLUDE REGEX "_")

    file(GLOB TESTS RELATIVE ${CMAKE_CURRENT_LIST_DIR} CONFIGURE_DEPENDS
      "${MACHINE}_*.sh")

    list(APPEND TESTS ${ALL_TESTS})

    foreach(TEST IN LISTS TESTS)
      string(REGEX REPLACE "\\.sh$" "" TESTNAME "${MACHINE}-${TEST}")

      add_test(NAME ${TESTNAME}
        COMMAND bash -x ${CMAKE_CURRENT_LIST_DIR}/${TEST}
        WORKING_DIRECTORY ${mold_BINARY_DIR})

      if(IS_NATIVE)
        set_tests_properties(${TESTNAME} PROPERTIES
          SKIP_REGULAR_EXPRESSION "skipped")
      else()
        set_tests_properties(${TESTNAME} PROPERTIES
          ENVIRONMENT "TRIPLE=${TRIPLE}")
      endif()
    endforeach()
  endif()
endfunction()

add_target(x86_64-linux-gnu)
add_target(i686-linux-gnu)
add_target(aarch64-linux-gnu)
add_target(arm-linux-gnueabihf)
add_target(riscv64-linux-gnu)
add_target(powerpc64-linux-gnu)
add_target(powerpc64le-linux-gnu)
add_target(sparc64-linux-gnu)
add_target(s390x-linux-gnu)
add_target(m68k-linux-gnu)
add_target(sh4-linux-gnu)

if(MOLD_ENABLE_QEMU_TESTS_RV32)
  add_target(riscv32-linux-gnu)
endif()
