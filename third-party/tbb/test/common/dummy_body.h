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

#ifndef __TBB_test_common_dummy_body_H
#define __TBB_test_common_dummy_body_H

#include "config.h"
#include <cstddef>

namespace utils {
static void doDummyWork(std::size_t N) {
    for (volatile std::size_t i = 0; i < N; ) { i = i + 1; }
}

//! Functor with N dummy iterations in it`s body
class DummyBody {
    int m_numIters;
public:
    explicit DummyBody( int iters = 0 ) : m_numIters( iters ) {}
    void operator()( int ) const {
        doDummyWork(m_numIters);
    }
    void operator()() const {
        doDummyWork(m_numIters);
    }
};

} // namespace utils

#endif // __TBB_test_common_dummy_body_H
