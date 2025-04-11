option(MOLD_ENABLE_QEMU_TESTS "Enable tests on non-native targets" ON)
set(MACHINE ${CMAKE_HOST_SYSTEM_PROCESSOR})

if(EXISTS "/proc/cpuinfo")
  file(READ "/proc/cpuinfo" CPUINFO)
endif()

# CMAKE_HOST_SYSTEM_PROCESSOR returns "aarch64" on ARM32 userland
# with ARM64 kernel. Here, we run `cc -dumpmachine` to determine the
# compiler's default target.
execute_process(COMMAND ${CMAKE_C_COMPILER} -dumpmachine
  RESULT_VARIABLE EXIT_CODE
  OUTPUT_VARIABLE ARCH
  ERROR_QUIET)
if(NOT EXIT_CODE AND ARCH MATCHES "([^-]+).*")
  set(MACHINE ${CMAKE_MATCH_1})
endif()

if(${MACHINE} MATCHES "amd64")
  set(MACHINE x86_64)
elseif(${MACHINE} MATCHES "i.86")
  set(MACHINE i686)
elseif(${MACHINE} MATCHES "arm.*")
  set(MACHINE arm)
elseif(${MACHINE} STREQUAL "powerpc64")
  set(MACHINE ppc64)
elseif(${MACHINE} STREQUAL "powerpc64le")
  set(MACHINE ppc64le)
endif()

if(MOLD_ENABLE_QEMU_TESTS)
  list(APPEND QEMU_ARCHS
    x86_64 i386 arm aarch64 aarch64_be ppc ppc64 ppc64le sparc64 sh4 sh4eb
    s390x riscv64 riscv32 m68k loongarch64 arc)

  LIST(APPEND TRIPLES
    x86_64-linux-gnu
    i686-linux-gnu
    aarch64-linux-gnu
    aarch64_be-linux-gnu
    arm-linux-gnueabihf
    riscv64-linux-gnu
    powerpc-linux-gnu
    powerpc64-linux-gnu
    powerpc64le-linux-gnu
    sparc64-linux-gnu
    s390x-linux-gnu
    sh4-linux-gnu
    sh4aeb-linux-gnu
    riscv32-linux-gnu
    m68k-linux-gnu
    loongarch64-linux-gnu
    arc-linux-gnu)

  foreach(ARCH IN LISTS QEMU_ARCHS)
    find_program(HAS_qemu-${ARCH} qemu-${ARCH})
  endforeach()

  foreach(TRIPLE IN LISTS TRIPLES)
    find_program(HAS_${TRIPLE}-gcc ${TRIPLE}-gcc)
  endforeach()
endif()

function(add_target ARCH TRIPLE)
  set(CPU ${ARGV2})

  if(${ARCH} STREQUAL ${MACHINE})
    set(IS_NATIVE 1)
  endif()

  file(GLOB ALL_TESTS RELATIVE ${CMAKE_CURRENT_LIST_DIR} CONFIGURE_DEPENDS
    "*.sh")

  list(FILTER ALL_TESTS EXCLUDE REGEX "^arch-")

  file(GLOB TESTS RELATIVE ${CMAKE_CURRENT_LIST_DIR} CONFIGURE_DEPENDS
    "arch-${ARCH}-*.sh")

  list(APPEND TESTS ${ALL_TESTS})

  foreach(TEST IN LISTS TESTS)
    if(CPU)
      string(REGEX REPLACE "\\.sh$" "" TESTNAME "${ARCH}_${CPU}-${TEST}")
    else()
      string(REGEX REPLACE "\\.sh$" "" TESTNAME "${ARCH}-${TEST}")
    endif()

    add_test(NAME ${TESTNAME}
      COMMAND bash -x ${CMAKE_CURRENT_LIST_DIR}/${TEST}
      WORKING_DIRECTORY ${mold_BINARY_DIR})

    set_property(TEST ${TESTNAME} APPEND PROPERTY ENVIRONMENT
      "MACHINE=${MACHINE};CPU=${CPU}")

    if(IS_NATIVE)
      set_property(TEST ${TESTNAME} APPEND PROPERTY SKIP_REGULAR_EXPRESSION
        "skipped")
    else()
      set_property(TEST ${TESTNAME} APPEND PROPERTY ENVIRONMENT
        "TRIPLE=${TRIPLE}")
    endif()
  endforeach()
endfunction()

if(${MACHINE} STREQUAL "x86_64" OR (HAS_qemu-x86_64 AND HAS_x86_64-linux-gnu-gcc))
  add_target(x86_64 x86_64-linux-gnu)
endif()

if(${MACHINE} STREQUAL "i686" OR (HAS_qemu-i386 AND HAS_i686-linux-gnu-gcc))
  add_target(i686 i686-linux-gnu)
endif()

if(${MACHINE} STREQUAL "aarch64" OR (HAS_qemu-aarch64 AND HAS_aarch64-linux-gnu-gcc))
  add_target(aarch64 aarch64-linux-gnu)
endif()

if(${MACHINE} STREQUAL "aarch64_be" OR (HAS_qemu-aarch64_be AND HAS_aarch64_be-linux-gnu-gcc))
  add_target(aarch64_be aarch64_be-linux-gnu)
endif()

if(${MACHINE} STREQUAL "arm" OR (HAS_qemu-arm AND HAS_arm-linux-gnueabihf-gcc))
  add_target(arm arm-linux-gnueabihf)
endif()

if(${MACHINE} STREQUAL "riscv64" OR (HAS_qemu-riscv64 AND HAS_riscv64-linux-gnu-gcc))
  add_target(riscv64 riscv64-linux-gnu)
endif()

if(${MACHINE} STREQUAL "riscv32" OR (HAS_qemu-riscv32 AND HAS_riscv32-linux-gnu-gcc))
  add_target(riscv32 riscv32-linux-gnu)
endif()

if(${MACHINE} STREQUAL "ppc" OR (HAS_qemu-ppc AND HAS_powerpc-linux-gnu-gcc))
  add_target(ppc powerpc-linux-gnu)
endif()

if(${MACHINE} STREQUAL "ppc64" OR (HAS_qemu-ppc64 AND HAS_powerpc64-linux-gnu-gcc))
  add_target(ppc64 powerpc64-linux-gnu)
endif()

if(${MACHINE} STREQUAL "ppc64le" OR (HAS_qemu-ppc64le AND HAS_powerpc64le-linux-gnu-gcc))
  add_target(ppc64le powerpc64le-linux-gnu)
endif()

if(${MACHINE} STREQUAL "ppc64le" AND "${CPUINFO}" MATCHES "POWER10")
  add_target(ppc64le powerpc64le-linux-gnu power10)
elseif(HAS_qemu-ppc64le AND HAS_powerpc64le-linux-gnu-gcc)
  file(WRITE "${CMAKE_BINARY_DIR}/empty.c" "")

  execute_process(
    COMMAND powerpc64le-linux-gnu-gcc -mcpu=power10 -E
      "${CMAKE_BINARY_DIR}/empty.c"
    RESULT_VARIABLE GCC_EXIT_CODE
    OUTPUT_QUIET ERROR_QUIET)

  execute_process(
    COMMAND qemu-ppc64le -cpu help
    OUTPUT_VARIABLE QEMU_CPUS)

  if("${GCC_EXIT_CODE}" EQUAL "0" AND "${QEMU_CPUS}" MATCHES power10_v2.0)
    add_target(ppc64le powerpc64le-linux-gnu power10)
  endif()
endif()

if(${MACHINE} STREQUAL "sparc64" OR (HAS_qemu-sparc64 AND HAS_sparc64-linux-gnu-gcc))
  add_target(sparc64 sparc64-linux-gnu)
endif()

if(${MACHINE} STREQUAL "s390x" OR (HAS_qemu-s390x AND HAS_s390x-linux-gnu-gcc))
  add_target(s390x s390x-linux-gnu)
endif()

if(${MACHINE} STREQUAL "sh4" OR (HAS_qemu-sh4 AND HAS_sh4-linux-gnu-gcc))
  add_target(sh4 sh4-linux-gnu)
endif()

if(${MACHINE} STREQUAL "sh4aeb" OR (HAS_qemu-sh4eb AND HAS_sh4aeb-linux-gnu-gcc))
  add_target(sh4aeb sh4aeb-linux-gnu)
endif()

if(${MACHINE} STREQUAL "m68k" OR (HAS_qemu-m68k AND HAS_m68k-linux-gnu-gcc))
  add_target(m68k m68k-linux-gnu)
endif()

if(${MACHINE} STREQUAL "loongarch64" OR (HAS_qemu-loongarch64 AND HAS_loongarch64-linux-gnu-gcc))
  add_target(loongarch64 loongarch64-linux-gnu)
endif()

if(${MACHINE} STREQUAL "arc" OR (HAS_qemu-loongarch64 AND HAS_arc-linux-gnu-gcc))
  add_target(arc arc-linux-gnu)
endif()
