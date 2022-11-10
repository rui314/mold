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

/**
    Evolution.h: Header file for evolution classes; evolution classes do
    looped evolution of patterns in a defined 2 dimensional space
**/

#ifndef TBB_examples_game_of_life_evolution_H
#define TBB_examples_game_of_life_evolution_H

#include <cstring>
#include <cstdlib>
#include <cstdio>

#include "oneapi/tbb/blocked_range.h"
#include "oneapi/tbb/parallel_for.h"
#include "oneapi/tbb/tick_count.h"
#include "oneapi/tbb/global_control.h"

#include "Board.hpp"

typedef unsigned int Int32;

void UpdateState(Matrix* m_matrix, char* dest, int begin, int end);

/**
    class Evolution - base class for SequentialEvolution and ParallelEvolution
**/
class Evolution {
public:
    Evolution(Matrix* m, //! beginning matrix including initial pattern
              BoardPtr board //! the board to update
              )
            : m_matrix(m),
              m_board(board),
              m_size(m_matrix->height * m_matrix->width),
              m_done(false) {
        //! allocate memory for second matrix data block
        m_dest = new char[m_size];
        is_paused = false;
    }

    virtual ~Evolution() {
        delete[] m_dest;
    }

    //! Run() - begins looped evolution
    virtual void Run(double execution_time, int nthread) = 0;

    //! Quit() - tell the thread to terminate
    virtual void Quit() {
        m_done = true;
    }

    //! Step() - performs a single evolutionary generation computation on the game matrix
    virtual void Step() = 0;

    //! SetPause() - change condition of variable is_paused
    virtual void SetPause(bool condition) {
        if (condition == true)
            is_paused = true;
        else
            is_paused = false;
    }

protected:
    /**
        UpdateMatrix() - moves the previous destination data to the source
        data block and zeros out destination.
    **/
    void UpdateMatrix();

protected:
    Matrix* m_matrix; //! Pointer to initial matrix
    char* m_dest; //! Pointer to calculation destination data
    BoardPtr m_board; //! The game board to update
    int m_size; //! size of the matrix data block
    volatile bool m_done; //! a flag used to terminate the thread
    Int32 m_nIteration; //! current calculation cycle index
    volatile bool is_paused; //! is needed to perform next iteration

    //! Calculation time of the sequential version (since the start), seconds.
    /**
        This member is updated by the sequential version and read by parallel,
        so no synchronization is necessary.
    **/
    double m_serial_time;
};

/**
    class SequentialEvolution - derived from Evolution - calculate life generations serially
**/
class SequentialEvolution : public Evolution {
public:
    SequentialEvolution(Matrix* m, BoardPtr board) : Evolution(m, board) {}
    virtual void Run(double execution_time, int nthread);
    virtual void Step();
};

/**
    class ParallelEvolution - derived from Evolution - calculate life generations
    in parallel using oneTBB
**/
class ParallelEvolution : public Evolution {
public:
    ParallelEvolution(Matrix* m, BoardPtr board) : Evolution(m, board), m_parallel_time(0) {
        // instantiate a global_control object and save a pointer to it
        m_pGlobControl = nullptr;
    }

    ~ParallelEvolution() {
        //! delete global_control object
        delete m_pGlobControl;
        m_pGlobControl = nullptr;
    }
    virtual void Run(double execution_time, int nthread);
    virtual void Step();

private:
    oneapi::tbb::global_control* m_pGlobControl;

    double m_parallel_time;
};

#endif /* TBB_examples_game_of_life_evolution_H */
