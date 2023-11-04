option(MOLD_ENABLE_QEMU_TESTS "Enable tests on non-native targets" OFF)

set(HOST ${CMAKE_HOST_SYSTEM_PROCESSOR})

# CMAKE_HOST_SYSTEM_PROCESSOR returns "aarch64" on ARM32 userland
# with ARM64 kernel. Here, we run `cc -dumpmachine` to determine the
# compiler's default target.
execute_process(COMMAND ${CMAKE_C_COMPILER} -dumpmachine
  RESULT_VARIABLE EXIT_CODE
  OUTPUT_VARIABLE ARCH
  ERROR_QUIET)
if(NOT EXIT_CODE AND ARCH MATCHES "([^-]+).*")
  set(HOST ${CMAKE_MATCH_1})
endif()

if(${HOST} MATCHES "amd64")
  set(HOST x86_64)
elseif(${HOST} MATCHES "i386")
  set(HOST i686)
elseif(${HOST} MATCHES "arm.*")
  set(HOST arm)
elseif(${HOST} STREQUAL "powerpc64")
  set(HOST ppc64)
elseif(${HOST} STREQUAL "powerpc64le")
  set(HOST ppc64le)
endif()

function(add_target ARCH TRIPLE)
  if(${ARCH} STREQUAL ${HOST})
    set(IS_NATIVE 1)
  endif()

  if(IS_NATIVE OR MOLD_ENABLE_QEMU_TESTS)
    file(GLOB ALL_TESTS RELATIVE ${CMAKE_CURRENT_LIST_DIR} CONFIGURE_DEPENDS "*.sh")

    list(FILTER ALL_TESTS EXCLUDE REGEX "_")

    file(GLOB TESTS RELATIVE ${CMAKE_CURRENT_LIST_DIR} CONFIGURE_DEPENDS "${ARCH}_*.sh")

    list(APPEND TESTS ${ALL_TESTS})

    foreach(TEST IN LISTS TESTS)
      string(REGEX REPLACE "\\.sh$" "" TESTNAME "${ARCH}-${TEST}")

      add_test(NAME ${TESTNAME}
        COMMAND bash -x ${CMAKE_CURRENT_LIST_DIR}/${TEST}
        WORKING_DIRECTORY ${mold_BINARY_DIR})

      set_property(TEST ${TESTNAME} APPEND PROPERTY ENVIRONMENT "HOST=${HOST}")

      if(IS_NATIVE)
        set_property(TEST ${TESTNAME} APPEND PROPERTY SKIP_REGULAR_EXPRESSION "skipped")
      else()
        set_property(TEST ${TESTNAME} APPEND PROPERTY ENVIRONMENT "TRIPLE=${TRIPLE}")
      endif()
    endforeach()
  endif()
endfunction()

add_target(x86_64 x86_64-linux-gnu)
add_target(i686 i686-linux-gnu)
add_target(aarch64 aarch64-linux-gnu)
add_target(arm arm-linux-gnueabihf)
add_target(riscv64 riscv64-linux-gnu)
add_target(ppc powerpc-linux-gnu)
add_target(ppc64 powerpc64-linux-gnu)
add_target(ppc64le powerpc64le-linux-gnu)
add_target(sparc64 sparc64-linux-gnu)
add_target(s390x s390x-linux-gnu)
add_target(sh4 sh4-linux-gnu)
add_target(alpha alpha-linux-gnu)

option(MOLD_ENABLE_QEMU_TESTS_RV32 "Enable tests for RV32" OFF)
if(MOLD_ENABLE_QEMU_TESTS_RV32)
  add_target(riscv32 riscv32-linux-gnu)
endif()

option(MOLD_ENABLE_QEMU_TESTS_POWER10 "Enable tests for Power10" OFF)
if(MOLD_ENABLE_QEMU_TESTS_POWER10)
  add_target(ppc64le powerpc64le_power10-linux-gnu)
endif()

option(MOLD_ENABLE_QEMU_TESTS_M68K "Enable tests for m68k" OFF)
if(MOLD_ENABLE_QEMU_TESTS_M68K)
  add_target(m68k m68k-linux-gnu)
endif()

option(MOLD_ENABLE_QEMU_TESTS_LOONGARCH "Enable tests for LoongArch" OFF)
if(MOLD_ENABLE_QEMU_TESTS_LOONGARCH OR ${HOST} STREQUAL loongarch64)
  add_target(loongarch64 loongarch64-linux-gnu)
endif()
