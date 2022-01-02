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

#ifndef TBB_examples_game_of_life_board_H
#define TBB_examples_game_of_life_board_H

#define LabelPtr int*
#define BoardPtr Board*

struct Matrix {
    int width;
    int height;
    char* data;
};

class Board {
public:
    Board(int width, int height, int squareSize, LabelPtr counter);
    virtual ~Board();
    void seed(int s);
    void seed(const BoardPtr s);

public:
    Matrix* m_matrix;

private:
    int m_width;
    int m_height;
    int m_squareSize;
    LabelPtr m_counter;
};

#endif /* TBB_examples_game_of_life_board_H */
