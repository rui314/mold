/*
    Copyright (c) 2005-2021 Intel Corporation

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

/*
    Evolution.cpp: implementation file for evolution classes; evolution
                  classes do looped evolution of patterns in a defined
                  2 dimensional space
*/

#include "common/utility/get_default_num_threads.hpp"

#include "Evolution.hpp"
#include "Board.hpp"

#ifdef USE_SSE
#define GRAIN_SIZE 14
#else
#define GRAIN_SIZE 4000
#endif
#define TIME_SLICE 330

/*
    Evolution
*/

/**
    Evolution::UpdateMatrix() - moves the calculated destination data
    to the source data block. No destination zeroing is required since it will
    be completely overwritten during the next calculation cycle.
**/
void Evolution::UpdateMatrix() {
    memcpy(m_matrix->data, m_dest, m_size);
}

/*
    SequentialEvolution
*/

//! SequentialEvolution::Run - begins looped evolution
void SequentialEvolution::Run(double execution_time, int nthread) {
    printf("Starting game (Sequential evolution)\n");

    m_nIteration = 0;
    m_serial_time = 0;
    oneapi::tbb::tick_count t0 = oneapi::tbb::tick_count::now();
    while (!m_done) {
        if (!is_paused) {
            oneapi::tbb::tick_count t = oneapi::tbb::tick_count::now();
            Step();
            oneapi::tbb::tick_count t1 = oneapi::tbb::tick_count::now();
            ++m_nIteration;
            double work_time = (t1 - t0).seconds();
            m_serial_time += work_time;
        }
        //! Let the parallel algorithm work uncontended almost the same time
        //! as the serial one. See ParallelEvolution::Run() as well.
        t0 = oneapi::tbb::tick_count::now();
        if (m_serial_time > execution_time) {
            printf("iterations count = %d time = %g\n", m_nIteration, m_serial_time);
            break;
        }
    }
}

//! SequentialEvolution::Step() - override of step method
void SequentialEvolution::Step() {
    if (!is_paused) {
#ifdef USE_SSE
        UpdateState(m_matrix, m_matrix->data, 0, m_matrix->height);
#else
        UpdateState(m_matrix, m_dest, 0, (m_matrix->width * m_matrix->height) - 1);
        UpdateMatrix();
#endif
    }
}

/*
    ParallelEvolution
*/

//! SequentialEvolution::Run - begins looped evolution
void ParallelEvolution::Run(double execution_time, int nthread) {
    if (nthread == utility::get_default_num_threads())
        printf("Starting game (Parallel evolution for automatic number of thread(s))\n");
    else
        printf("Starting game (Parallel evolution for %d thread(s))\n", nthread);

    m_nIteration = 0;
    m_parallel_time = 0;

    oneapi::tbb::global_control* pGlobControl = new oneapi::tbb::global_control(
        oneapi::tbb::global_control::max_allowed_parallelism, nthread);

    double work_time = m_serial_time;
    oneapi::tbb::tick_count t0 = oneapi::tbb::tick_count::now();

    while (!m_done) {
        if (!is_paused) {
            oneapi::tbb::tick_count t = oneapi::tbb::tick_count::now();
            Step();
            oneapi::tbb::tick_count t1 = oneapi::tbb::tick_count::now();
            ++m_nIteration;
            double real_work_time = (t1 - t0).seconds();
            m_parallel_time += real_work_time;
        }
        //! Let the serial algorithm work the same time as the parallel one.
        t0 = oneapi::tbb::tick_count::now();
        if (m_parallel_time > execution_time) {
            printf("iterations count = %d time = %g\n", m_nIteration, m_parallel_time);
            delete pGlobControl;
            pGlobControl = nullptr;
            break;
        }
    }
    delete pGlobControl;
    pGlobControl = nullptr;
}

/**
    class tbb_parallel_task

    TBB requires a class for parallel loop implementations. The actual
    loop "chunks" are performed using the () operator of the class.
    The blocked_range contains the range to calculate. Please see the
    TBB documentation for more information.
**/
class tbb_parallel_task {
public:
    static void set_values(Matrix* source, char* dest) {
        m_source = source;
        m_dest = dest;
        return;
    }

    void operator()(const oneapi::tbb::blocked_range<std::size_t>& r) const {
        int begin = (int)r.begin(); //! capture lower range number for this chunk
        int end = (int)r.end(); //! capture upper range number for this chunk
        UpdateState(m_source, m_dest, begin, end);
    }

    tbb_parallel_task() {}

private:
    static Matrix* m_source;
    static char* m_dest;
};

Matrix* tbb_parallel_task::m_source;
char* tbb_parallel_task::m_dest;

//! ParallelEvolution::Step() - override of Step method
void ParallelEvolution::Step() {
    std::size_t begin = 0; //! beginning cell position
#ifdef USE_SSE
    std::size_t end = m_matrix->height; //! ending cell position
#else
    std::size_t end = m_size - 1; //! ending cell position
#endif

    //! set matrix pointers
    tbb_parallel_task::set_values(m_matrix, m_dest);

    //! do calculation loop
    parallel_for(oneapi::tbb::blocked_range<std::size_t>(begin, end, GRAIN_SIZE),
                 tbb_parallel_task());
    UpdateMatrix();
}
