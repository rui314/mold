#include <cstdint>
#include <vector>
#include <omp.h>
#ifndef _WIN32
#include <pthread.h>
#endif
#include <oneapi/tbb/global_control.h>
#include <oneapi/tbb/parallel_for.h>

namespace nesting_tbb {

/*begin outer loop openmp with nested tbb*/
int M, N;

struct InnerBody {
    int i;
    void operator()(tbb::blocked_range<int> const& r) const {
        for (auto j = r.begin(); j != r.end(); ++j) {
            // do the work for (i, j) element
        }
    }
};

void TBB_NestedInOpenMP() {
#pragma omp parallel
    {
#pragma omp for
        for(int i = 0; i < M; ++i) {
            tbb::parallel_for(tbb::blocked_range<int>(0, N, 10), InnerBody(i));
        }
    }
}
/*end outer loop openmp with nested tbb*/

void test() {
    M = 2; N = 100;
    TBB_NestedInOpenMP();
}

} // namespace nesting_tbb

#ifndef _WIN32
namespace pthreads_and_tbb {

/*begin pthreads with tbb*/
int M, N;

struct InnerBody {
    int i;
    void operator()(tbb::blocked_range<int> const& r) const {
        for (auto j = r.begin(); j != r.end(); ++j) {
            // do the work for (i, j) element
        }
    }
};

void* OuterLoopIteration(void* args) {
    int i = reinterpret_cast<intptr_t>(args);
    tbb::parallel_for(tbb::blocked_range<int>(0, N, 10), InnerBody(i));
    return nullptr;
}

void TBB_NestedInPThreads() {
    std::vector<pthread_t> id(M);
    // Create thread for each outer loop iteration
    for(int i = 0; i < M; ++i) {
        std::intptr_t arg = i;
        pthread_create(&id[i], NULL, OuterLoopIteration, (void*)arg);
    }
    // Wait for outer loop threads to finish
    for(int i = 0; i < M; ++i)
        pthread_join(id[i], NULL);
}
/*end pthreads with tbb*/

void test() {
    M = 2; N = 100;
    TBB_NestedInPThreads();
}

} // namespace pthreads_and_tbb
#endif                          // _WIN32

namespace nesting_omp {

/*begin outer loop tbb with nested omp*/
int M, N;

void InnerBody(int i, int j) {
    // do the work for (i, j) element
}

void OpenMP_NestedInTBB() {
    tbb::parallel_for(0, M, [&](int i) {
        #pragma omp parallel for
        for(int j = 0; j < N; ++j) {
            InnerBody(i, j);
        }
    });
}
/*end outer loop tbb with nested omp*/

void test() {
    M = 2; N = 100;
    OpenMP_NestedInTBB();
}

} // namespace nesting_omp


int main() {
    // Setting maximum number of threads for both runtimes to avoid
    // oversubscription issues
    constexpr int max_threads = 2;
    omp_set_num_threads(max_threads);
    tbb::global_control gl(tbb::global_control::max_allowed_parallelism, max_threads);

    nesting_tbb::test();
#ifndef _WIN32
    pthreads_and_tbb::test();
#endif
    nesting_omp::test();
}
