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

/* 
    Game_of_life.cpp : 
                      main project file.
*/

#include <ctime>

#include <iostream>
#include <sstream>

#include "common/utility/get_default_num_threads.hpp"

#include "Board.hpp"
#include "Evolution.hpp"

#define BOARD_SQUARE_SIZE 2

int low; //! lower range limit of threads
int high; //! high range limit of threads
double execution_time; //! time for game of life iterations

Board::Board(int width, int height, int squareSize, LabelPtr counter)
        : m_width(width),
          m_height(height),
          m_squareSize(squareSize),
          m_counter(counter) {
    m_matrix = new Matrix();
    m_matrix->width = width;
    m_matrix->height = height;
    m_matrix->data = new char[width * height];
    memset(m_matrix->data, 0, width * height);
}

Board::~Board() {
    delete[] m_matrix->data;
    delete m_matrix;
}

void Board::seed(int s) {
    srand(s);
    for (int j = 0; j < m_height; j++) {
        for (int i = 0; i < m_width; i++) {
            int x = rand() / (int)(((unsigned)RAND_MAX + 1) / 100);
            m_matrix->data[i + j * m_width] = x > 75 ? 1 : 0; // 25% occupied
        }
    }
}

void Board::seed(const BoardPtr src) {
    memcpy(m_matrix->data, src->m_matrix->data, m_height * m_width);
}

//! Print usage of this program
void PrintUsage() {
    printf("Usage: game_of_life [M[:N] -t execution_time]\n"
           "M and N are a range of numbers of threads to be used.\n"
           "execution_time is a time (in sec) for execution game_of_life iterations\n");
    printf("Default values:\n"
           "M:\t\tautomatic\n"
           "N:\t\tM\n"
           "execution_time:\t10\n");
}

//! Parse command line
bool ParseCommandLine(int argc, char* argv[]) {
    char* s = argv[1];
    char* end;
    //! command line without parameters
    if (argc == 1) {
        low = utility::get_default_num_threads();
        high = low;
        execution_time = 5;
        return true;
    }
    //! command line with parameters
    if (argc != 4) {
        PrintUsage();
        return false;
    }
    if (std::string("-t") != argv[argc - 2])
        //! process M[:N] parameter
        high = strtol(s, &end, 0);
    low = strtol(s, &end, 0);
    switch (*end) {
        case ':': high = strtol(end + 1, nullptr, 0); break;
        case '\0': break;
        default: PrintUsage(); return false;
    }
    if (high < low) {
        std::cout << "Set correct range. Current range: " << low << ":" << high << "\n";
        PrintUsage();
        return false;
    }
    //! process execution_time parameter
    execution_time = strtol(argv[argc - 1], &end, 0);
    return true;
}

int main(int argc, char* argv[]) {
    if (!ParseCommandLine(argc, argv))
        return -1;
    SequentialEvolution* m_seq;
    ParallelEvolution* m_par;
    Board* m_board1;
    Board* m_board2;
    int* count = nullptr;

    int boardWidth = 300;
    int boardHeight = 300;

    m_board1 = new Board(boardWidth, boardHeight, BOARD_SQUARE_SIZE, count);
    m_board2 = new Board(boardWidth, boardHeight, BOARD_SQUARE_SIZE, count);

    time_t now = time(nullptr);
    printf("Generate Game of life board\n");
    m_board1->seed((int)now);
    m_board2->seed(m_board1);

    m_seq = new SequentialEvolution(m_board1->m_matrix, m_board1);
    m_seq->Run(execution_time, 1);
    delete m_seq;

    m_par = new ParallelEvolution(m_board2->m_matrix, m_board2);
    for (int p = low; p <= high; ++p) {
        m_par->Run(execution_time, p);
    }
    delete m_par;

    delete m_board1;
    delete m_board2;
    return 0;
}
