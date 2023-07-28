/*
    Copyright (c) 2005-2022 Intel Corporation

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.
*/

#define DOCTEST_CONFIG_IMPLEMENT
#include "common/test.h"

#include "common/utils.h"
#include "common/utils_report.h"

#include "tbb/global_control.h"
#include "tbb/task_arena.h"
#include "tbb/scalable_allocator.h"


// Lets slow down the main thread on exit
static constexpr int MAX_DELAY = 5;
struct GlobalObject {
    ~GlobalObject() {
        utils::Sleep(rand( ) % MAX_DELAY);
    }
} go;

void allocatorRandomThrashing() {
    const int ARRAY_SIZE = 1000;
    const int MAX_ITER = 10000;
    const int MAX_ALLOC = 10 * 1024 * 1024;

    void *arr[ARRAY_SIZE] = {nullptr};
    for (int i = 0; i < rand() % MAX_ITER; ++i) {
        // Random allocation size for random arrays
        for (int j = 0; j < rand() % ARRAY_SIZE; ++j) {
            arr[j] = scalable_malloc(rand() % MAX_ALLOC);
        }
        // Deallocate everything
        for (int j = 0; j < ARRAY_SIZE; ++j) {
            scalable_free(arr[j]);
            arr[j] = nullptr;
        }
    }
}

void hangOnExitReproducer() {
    const int P = tbb::global_control::max_allowed_parallelism;
    tbb::task_arena test_arena;
    for (int i = 0; i < P-1; i++) {
        test_arena.enqueue(allocatorRandomThrashing);
    }
}

#if (_WIN32 || _WIN64) && !__TBB_WIN8UI_SUPPORT
#include <process.h> // _spawnl
void processSpawn(const char* self) {
    _spawnl(_P_WAIT, self, self, "1", nullptr);
}
#elif __unix__ || __APPLE__
#include <unistd.h> // fork/exec
#include <sys/wait.h> // waitpid
void processSpawn(const char* self) {
    pid_t pid = fork();
    if (pid == -1) {
        REPORT("ERROR: fork failed.\n");
    } else if (pid == 0) { // child
        execl(self, self, "1", nullptr);
        REPORT("ERROR: exec never returns\n");
        exit(1);
    } else { // parent
        int status;
        waitpid(pid, &status, 0);
    }
}
#else
void processSpawn(const char* /*self*/) {
    REPORT("Known issue: no support for process spawn on this platform.\n");
    REPORT("done\n");
    exit(0);
}
#endif

#if _MSC_VER && !__INTEL_COMPILER
#pragma warning (push)
#pragma warning (disable: 4702)  /* Unreachable code */
#endif

//! \brief \ref error_guessing
TEST_CASE("testing shutdown hang") {
    hangOnExitReproducer();
    CHECK(true); // just to notify that test has assertions
}

DOCTEST_MSVC_SUPPRESS_WARNING_WITH_PUSH(4007) // 'function' : must be 'attribute' - see issue #182
DOCTEST_MSVC_SUPPRESS_WARNING_WITH_PUSH(4447)
int main(int argc, char* argv[]) {
    // Executed from child processes
    if (argc == 2 && strcmp(argv[1],"1") == 0) {
        return doctest::Context(argc, argv).run();
    }

    // The number of executions is a tradeoff
    // between execution time and NBTS statistics
    const int EXEC_TIMES = 100;
    const char* self = argv[0];
    for (int i = 0; i < EXEC_TIMES; i++) {
        processSpawn(self);
    }

#if _MSC_VER && !__INTEL_COMPILER
#pragma warning (pop)
#endif
}
DOCTEST_MSVC_SUPPRESS_WARNING_POP
