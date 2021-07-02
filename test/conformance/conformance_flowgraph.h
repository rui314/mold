/*
    Copyright (c) 2020-2021 Intel Corporation

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

#ifndef __TBB_test_conformance_conformance_flowgraph_H
#define __TBB_test_conformance_conformance_flowgraph_H

struct passthru_body {
    int operator()( int i ) {
        return i;
    }
    void operator()( const int& argument, oneapi::tbb::flow::multifunction_node<int, std::tuple<int>>::output_ports_type &op ) {
       std::get<0>(op).try_put(argument);
    }

};

template<typename V>
using test_push_receiver = oneapi::tbb::flow::queue_node<V>;

template<typename V>
int get_count( test_push_receiver<V>& rr ){
    int val = 0;
    for(V tmp; rr.try_get(tmp); ++val);

    return val;
}


template< typename OutputType >
struct first_functor {
    int my_id;
    static std::atomic<int> first_id;

    first_functor(int id) : my_id(id) {}

    OutputType operator()( OutputType argument ) {
        int old_value = first_id;
        while(first_id == -1 &&
              !first_id.compare_exchange_weak(old_value, my_id))
            ;

        return argument;
    }

    OutputType operator()( const oneapi::tbb::flow::continue_msg&  ) {
        return operator()(OutputType());
    }

    void operator()( const OutputType& argument, oneapi::tbb::flow::multifunction_node<int, std::tuple<int>>::output_ports_type &op ) {
        operator()(OutputType());
        std::get<0>(op).try_put(argument);
    }
};

template<typename OutputType>
std::atomic<int> first_functor<OutputType>::first_id;


template< typename OutputType >
struct inc_functor {
    static std::atomic<size_t> execute_count;

    OutputType operator()( oneapi::tbb::flow::continue_msg ) {
       ++execute_count;
       return OutputType();
    }

    OutputType operator()( int argument ) {
       ++execute_count;
       return argument;
    }
};

template<typename OutputType>
std::atomic<size_t> inc_functor<OutputType>::execute_count;

#endif // __TBB_test_conformance_conformance_flowgraph_H
