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

#if __INTEL_COMPILER && _MSC_VER
#pragma warning(disable : 2586) // decorated name length exceeded, name was truncated
#endif

#include "common/config.h"

#include "tbb/flow_graph.h"

#include "common/test.h"
#include "common/utils.h"
#include "common/utils_assert.h"
#include "common/test_follows_and_precedes_api.h"


//! \file test_indexer_node.cpp
//! \brief Test for [flow_graph.indexer_node] specification


#if defined(_MSC_VER) && _MSC_VER < 1600
    #pragma warning (disable : 4503) //disabling the "decorated name length exceeded" warning for VS2008 and earlier
#endif

const int Count = 150;
const int MaxPorts = 10;
const int MaxNInputs = 5; // max # of input_nodes to register for each indexer_node input in parallel test
bool outputCheck[MaxPorts][Count];  // for checking output

void
check_outputCheck( int nUsed, int maxCnt) {
    for(int i=0; i < nUsed; ++i) {
        for( int j = 0; j < maxCnt; ++j) {
            CHECK_MESSAGE(outputCheck[i][j], "");
        }
    }
}

void
reset_outputCheck( int nUsed, int maxCnt) {
    for(int i=0; i < nUsed; ++i) {
        for( int j = 0; j < maxCnt; ++j) {
            outputCheck[i][j] = false;
        }
    }
}

class test_class {
    public:
        test_class() { my_val = 0; }
        test_class(int i) { my_val = i; }
        operator int() { return my_val; }
    private:
        int my_val;
};

template<typename T>
class name_of {
public:
    static const char* name() { return  "Unknown"; }
};
template<>
class name_of<int> {
public:
    static const char* name() { return  "int"; }
};
template<>
class name_of<float> {
public:
    static const char* name() { return  "float"; }
};
template<>
class name_of<double> {
public:
    static const char* name() { return  "double"; }
};
template<>
class name_of<long> {
public:
    static const char* name() { return  "long"; }
};
template<>
class name_of<short> {
public:
    static const char* name() { return  "short"; }
};
template<>
class name_of<test_class> {
public:
    static const char* name() { return  "test_class"; }
};

// TT must be arithmetic, and shouldn't wrap around for reasonable sizes of Count (which is now 150, and maxPorts is 10,
// so the max number generated right now is 1500 or so.)  Input will generate a series of TT with value
// (init_val + (i-1)*addend) * my_mult, where i is the i-th invocation of the body.  We are attaching addend
// input nodes to a indexer_port, and each will generate part of the numerical series the port is expecting
// to receive.  If there is only one input node, the series order will be maintained; if more than one,
// this is not guaranteed.
// The manual specifies bodies can be assigned, so we can't hide the operator=.
template<typename TT>
class my_input_body {
    TT my_mult;
    int my_count;
    int addend;
public:
    my_input_body(TT multiplier, int init_val, int addto) : my_mult(multiplier), my_count(init_val), addend(addto) { }
    TT operator()( tbb::flow_control& fc) {
        int lc = my_count;
        TT ret = my_mult * (TT)my_count;
        my_count += addend;
        if ( lc < Count){
            return ret;
        }else{
            fc.stop();
            return TT();
        }
    }
};

// allocator for indexer_node.

template<typename IType>
class makeIndexer {
public:
    static IType *create() {
        IType *temp = new IType();
        return temp;
    }
    static void destroy(IType *p) { delete p; }
};

template<int ELEM, typename INT>
struct getval_helper {

    typedef typename INT::output_type OT;
    typedef typename std::tuple_element<ELEM-1, typename INT::tuple_types>::type stored_type;

    static int get_integer_val(OT const &o) {
        stored_type res = tbb::flow::cast_to<stored_type>(o);
        return (int)res;
    }
};

// holder for input_node pointers for eventual deletion

static void* all_input_nodes[MaxPorts][MaxNInputs];

template<int ELEM, typename INT>
class input_node_helper {
public:
    typedef INT indexer_node_type;
    typedef typename indexer_node_type::output_type TT;
    typedef typename std::tuple_element<ELEM-1,typename INT::tuple_types>::type IT;
    typedef typename tbb::flow::input_node<IT> my_input_node_type;
    static void print_remark() {
        input_node_helper<ELEM-1,INT>::print_remark();
        INFO(", " << name_of<IT>::name());
    }
    static void add_input_nodes(indexer_node_type &my_indexer, tbb::flow::graph &g, int nInputs) {
        for(int i=0; i < nInputs; ++i) {
            my_input_node_type *new_node = new my_input_node_type(g, my_input_body<IT>((IT)(ELEM+1), i, nInputs));
            tbb::flow::make_edge(*new_node, tbb::flow::input_port<ELEM-1>(my_indexer));

            all_input_nodes[ELEM-1][i] = (void *)new_node;
            new_node->activate();
        }

        // add the next input_node
        input_node_helper<ELEM-1, INT>::add_input_nodes(my_indexer, g, nInputs);
    }
    static void check_value(TT &v) {
        if(v.tag() == ELEM-1) {
            int ival = getval_helper<ELEM,INT>::get_integer_val(v);
            CHECK_MESSAGE(!(ival%(ELEM+1)), "");
            ival /= (ELEM+1);
            CHECK_MESSAGE(!outputCheck[ELEM-1][ival], "");
            outputCheck[ELEM-1][ival] = true;
        }
        else {
            input_node_helper<ELEM-1,INT>::check_value(v);
        }
    }

    static void remove_input_nodes(indexer_node_type& my_indexer, int nInputs) {
        for(int i=0; i< nInputs; ++i) {
            my_input_node_type *dp = reinterpret_cast<my_input_node_type *>(all_input_nodes[ELEM-1][i]);
            tbb::flow::remove_edge(*dp, tbb::flow::input_port<ELEM-1>(my_indexer));
            delete dp;
        }
        input_node_helper<ELEM-1, INT>::remove_input_nodes(my_indexer, nInputs);
    }
};

template<typename INT>
class input_node_helper<1, INT> {
    typedef INT indexer_node_type;
    typedef typename indexer_node_type::output_type TT;

    typedef typename std::tuple_element<0, typename INT::tuple_types>::type IT;
    typedef typename tbb::flow::input_node<IT> my_input_node_type;
public:
    static void print_remark() {
        INFO("Parallel test of indexer_node< " << name_of<IT>::name());
    }
    static void add_input_nodes(indexer_node_type &my_indexer, tbb::flow::graph &g, int nInputs) {
        for(int i=0; i < nInputs; ++i) {
            my_input_node_type *new_node = new my_input_node_type(g, my_input_body<IT>((IT)2, i, nInputs));
            tbb::flow::make_edge(*new_node, tbb::flow::input_port<0>(my_indexer));
            all_input_nodes[0][i] = (void *)new_node;
            new_node->activate();
        }
    }
    static void check_value(TT &v) {
        int ival = getval_helper<1,INT>::get_integer_val(v);
        CHECK_MESSAGE(!(ival%2), "");
        ival /= 2;
        CHECK_MESSAGE(!outputCheck[0][ival], "");
        outputCheck[0][ival] = true;
    }
    static void remove_input_nodes(indexer_node_type& my_indexer, int nInputs) {
        for(int i=0; i < nInputs; ++i) {
            my_input_node_type *dp = reinterpret_cast<my_input_node_type *>(all_input_nodes[0][i]);
            tbb::flow::remove_edge(*dp, tbb::flow::input_port<0>(my_indexer));
            delete dp;
        }
    }
};

template<typename IType>
class parallel_test {
public:
    typedef typename IType::output_type TType;
    typedef typename IType::tuple_types union_types;
    static const int SIZE = std::tuple_size<union_types>::value;
    static void test() {
        TType v;
        input_node_helper<SIZE,IType>::print_remark();
        INFO(" >\n");
        for(int i=0; i < MaxPorts; ++i) {
            for(int j=0; j < MaxNInputs; ++j) {
                all_input_nodes[i][j] = nullptr;
            }
        }
        for(int nInputs = 1; nInputs <= MaxNInputs; ++nInputs) {
            tbb::flow::graph g;
            IType* my_indexer_ptr = new IType(g); //makeIndexer<IType>::create();
            IType my_indexer = *my_indexer_ptr;
            tbb::flow::queue_node<TType> outq1(g);
            tbb::flow::queue_node<TType> outq2(g);

            tbb::flow::make_edge(my_indexer, outq1);
            tbb::flow::make_edge(my_indexer, outq2);

            input_node_helper<SIZE, IType>::add_input_nodes(my_indexer, g, nInputs);

            g.wait_for_all();
            makeIndexer<IType>::destroy(my_indexer_ptr);

            reset_outputCheck(SIZE, Count);
            for(int i=0; i < Count*SIZE; ++i) {
                CHECK_MESSAGE(outq1.try_get(v), "");
                input_node_helper<SIZE, IType>::check_value(v);
            }

            check_outputCheck(SIZE, Count);
            reset_outputCheck(SIZE, Count);

            for(int i=0; i < Count*SIZE; i++) {
                CHECK_MESSAGE(outq2.try_get(v), "");
                input_node_helper<SIZE, IType>::check_value(v);
            }
            check_outputCheck(SIZE, Count);

            CHECK_MESSAGE(!outq1.try_get(v), "");
            CHECK_MESSAGE(!outq2.try_get(v), "");

            input_node_helper<SIZE, IType>::remove_input_nodes(my_indexer, nInputs);
            tbb::flow::remove_edge(my_indexer, outq1);
            tbb::flow::remove_edge(my_indexer, outq2);
        }
    }
};

std::vector<int> last_index_seen;

template<int ELEM, typename IType>
class serial_queue_helper {
public:
    typedef typename IType::output_type OT;
    typedef typename IType::tuple_types TT;
    typedef typename std::tuple_element<ELEM-1,TT>::type IT;
    static void print_remark() {
        serial_queue_helper<ELEM-1,IType>::print_remark();
        INFO("," << name_of<IT>::name());
    }
    static void fill_one_queue(int maxVal, IType &my_indexer) {
        // fill queue to "left" of me
        serial_queue_helper<ELEM-1,IType>::fill_one_queue(maxVal,my_indexer);
        for(int i = 0; i < maxVal; ++i) {
            CHECK_MESSAGE(tbb::flow::input_port<ELEM-1>(my_indexer).try_put((IT)(i*(ELEM+1))), "");
        }
    }
    static void put_one_queue_val(int myVal, IType &my_indexer) {
        // put this val to my "left".
        serial_queue_helper<ELEM-1,IType>::put_one_queue_val(myVal, my_indexer);
        CHECK_MESSAGE(tbb::flow::input_port<ELEM-1>(my_indexer).try_put((IT)(myVal*(ELEM+1))), "");
    }
    static void check_queue_value(OT &v) {
        if(ELEM - 1 == v.tag()) {
            // this assumes each or node input is queueing.
            int rval = getval_helper<ELEM,IType>::get_integer_val(v);
            CHECK_MESSAGE( rval == (last_index_seen[ELEM-1]+1)*(ELEM+1), "");
            last_index_seen[ELEM-1] = rval / (ELEM+1);
        }
        else {
            serial_queue_helper<ELEM-1,IType>::check_queue_value(v);
        }
    }
};

template<typename IType>
class serial_queue_helper<1, IType> {
public:
    typedef typename IType::output_type OT;
    typedef typename IType::tuple_types TT;
    typedef typename std::tuple_element<0,TT>::type IT;
    static void print_remark() {
        INFO("Serial test of indexer_node< " << name_of<IT>::name());
    }
    static void fill_one_queue(int maxVal, IType &my_indexer) {
        for(int i = 0; i < maxVal; ++i) {
            CHECK_MESSAGE(tbb::flow::input_port<0>(my_indexer).try_put((IT)(i*2)), "");
        }
    }
    static void put_one_queue_val(int myVal, IType &my_indexer) {
        CHECK_MESSAGE(tbb::flow::input_port<0>(my_indexer).try_put((IT)(myVal*2)), "");
    }
    static void check_queue_value(OT &v) {
        CHECK_MESSAGE(v.tag() == 0, "");  // won't get here unless true
        int rval = getval_helper<1,IType>::get_integer_val(v);
        CHECK_MESSAGE( rval == (last_index_seen[0]+1)*2, "");
        last_index_seen[0] = rval / 2;
    }
};

template<typename IType, typename TType, int SIZE>
void test_one_serial( IType &my_indexer, tbb::flow::graph &g) {
    last_index_seen.clear();
    for(int ii=0; ii < SIZE; ++ii) last_index_seen.push_back(-1);

    typedef TType q3_input_type;
    tbb::flow::queue_node< q3_input_type >  q3(g);
    q3_input_type v;

    tbb::flow::make_edge(my_indexer, q3);

    // fill each queue with its value one-at-a-time
    for (int i = 0; i < Count; ++i ) {
        serial_queue_helper<SIZE,IType>::put_one_queue_val(i,my_indexer);
    }

    g.wait_for_all();
    for (int i = 0; i < Count * SIZE; ++i ) {
        g.wait_for_all();
        CHECK_MESSAGE( (q3.try_get( v )), "Error in try_get()");
        {
            serial_queue_helper<SIZE,IType>::check_queue_value(v);
        }
    }
    CHECK_MESSAGE( (!q3.try_get( v )), "extra values in output queue");
    for(int ii=0; ii < SIZE; ++ii) last_index_seen[ii] = -1;

    // fill each queue completely before filling the next.
    serial_queue_helper<SIZE, IType>::fill_one_queue(Count,my_indexer);

    g.wait_for_all();
    for (int i = 0; i < Count*SIZE; ++i ) {
        g.wait_for_all();
        CHECK_MESSAGE( (q3.try_get( v )), "Error in try_get()");
        {
            serial_queue_helper<SIZE,IType>::check_queue_value(v);
        }
    }
    CHECK_MESSAGE( (!q3.try_get( v )), "extra values in output queue");
}

//
template<typename NodeType>
void test_input_ports_return_ref(NodeType& mip_node) {
    typename NodeType::input_ports_type& input_ports1 = mip_node.input_ports();
    typename NodeType::input_ports_type& input_ports2 = mip_node.input_ports();
    CHECK_MESSAGE( (&input_ports1 == &input_ports2), "input_ports() should return reference");
}

// Single predecessor at each port, single accepting successor
//   * put to buffer before port0, then put to buffer before port1, ...
//   * fill buffer before port0 then fill buffer before port1, ...

template<typename IType>
class serial_test {
    typedef typename IType::output_type TType;  // this is the union
    typedef typename IType::tuple_types union_types;
    static const int SIZE = std::tuple_size<union_types>::value;
public:
static void test() {
    tbb::flow::graph g;
    static const int ELEMS = 3;
    IType* my_indexer = new IType(g); //makeIndexer<IType>::create(g);

    test_input_ports_return_ref(*my_indexer);

    serial_queue_helper<SIZE, IType>::print_remark(); INFO(" >\n");

    test_one_serial<IType,TType,SIZE>(*my_indexer, g);

    std::vector<IType> indexer_vector(ELEMS,*my_indexer);

    makeIndexer<IType>::destroy(my_indexer);

    for(int e = 0; e < ELEMS; ++e) {
        test_one_serial<IType,TType,SIZE>(indexer_vector[e], g);
    }
}

}; // serial_test

template<
      template<typename> class TestType,  // serial_test or parallel_test
      typename T0, typename T1=void, typename T2=void, typename T3=void, typename T4=void,
      typename T5=void, typename T6=void, typename T7=void, typename T8=void, typename T9=void> // type of the inputs to the indexer_node
class generate_test {
public:
    typedef tbb::flow::indexer_node<T0, T1, T2, T3, T4, T5, T6, T7, T8, T9>  indexer_node_type;
    static void do_test() {
        TestType<indexer_node_type>::test();
    }
};

//specializations for indexer node inputs
template<
      template<typename> class TestType,
      typename T0, typename T1, typename T2, typename T3, typename T4,
      typename T5, typename T6, typename T7, typename T8>
class generate_test<TestType, T0, T1, T2, T3, T4, T5, T6, T7, T8> {
public:
    typedef tbb::flow::indexer_node<T0, T1, T2, T3, T4, T5, T6, T7, T8>  indexer_node_type;
    static void do_test() {
        TestType<indexer_node_type>::test();
    }
};

template<
      template<typename> class TestType,
      typename T0, typename T1, typename T2, typename T3, typename T4,
      typename T5, typename T6, typename T7>
class generate_test<TestType, T0, T1, T2, T3, T4, T5, T6, T7> {
public:
    typedef tbb::flow::indexer_node<T0, T1, T2, T3, T4, T5, T6, T7>  indexer_node_type;
    static void do_test() {
        TestType<indexer_node_type>::test();
    }
};

template<
      template<typename> class TestType,
      typename T0, typename T1, typename T2, typename T3, typename T4,
      typename T5, typename T6>
class generate_test<TestType, T0, T1, T2, T3, T4, T5, T6> {
public:
    typedef tbb::flow::indexer_node<T0, T1, T2, T3, T4, T5, T6>  indexer_node_type;
    static void do_test() {
        TestType<indexer_node_type>::test();
    }
};

template<
      template<typename> class TestType,
      typename T0, typename T1, typename T2, typename T3, typename T4,
      typename T5>
class generate_test<TestType, T0, T1, T2, T3, T4, T5>  {
public:
    typedef tbb::flow::indexer_node<T0, T1, T2, T3, T4, T5>  indexer_node_type;
    static void do_test() {
        TestType<indexer_node_type>::test();
    }
};

template<
      template<typename> class TestType,
      typename T0, typename T1, typename T2, typename T3, typename T4>
class generate_test<TestType, T0, T1, T2, T3, T4>  {
public:
    typedef tbb::flow::indexer_node<T0, T1, T2, T3, T4>  indexer_node_type;
    static void do_test() {
        TestType<indexer_node_type>::test();
    }
};

template<
      template<typename> class TestType,
      typename T0, typename T1, typename T2, typename T3>
class generate_test<TestType, T0, T1, T2, T3> {
public:
    typedef tbb::flow::indexer_node<T0, T1, T2, T3>  indexer_node_type;
    static void do_test() {
        TestType<indexer_node_type>::test();
    }
};

template<
      template<typename> class TestType,
      typename T0, typename T1, typename T2>
class generate_test<TestType, T0, T1, T2> {
public:
    typedef tbb::flow::indexer_node<T0, T1, T2>  indexer_node_type;
    static void do_test() {
        TestType<indexer_node_type>::test();
    }
};

template<
      template<typename> class TestType,
      typename T0, typename T1>
class generate_test<TestType, T0, T1> {
public:
    typedef tbb::flow::indexer_node<T0, T1>  indexer_node_type;
    static void do_test() {
        TestType<indexer_node_type>::test();
    }
};

template<
      template<typename> class TestType,
      typename T0>
class generate_test<TestType, T0> {
public:
    typedef tbb::flow::indexer_node<T0>  indexer_node_type;
    static void do_test() {
        TestType<indexer_node_type>::test();
    }
};

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
template<typename tagged_msg_t, typename input_t>
bool check_edge(tbb::flow::graph& g,
                tbb::flow::broadcast_node<input_t>& start,
                tbb::flow::buffer_node<tagged_msg_t>& buf,
                input_t input_value) {
    start.try_put(input_value);
    g.wait_for_all();

    tagged_msg_t msg;
    bool is_get_succeeded = buf.try_get(msg);

    CHECK_MESSAGE( ((is_get_succeeded)), "There is no item in the buffer");
    CHECK_MESSAGE( ((tbb::flow::cast_to<input_t>(msg) == input_value)), "Wrong item value");
    return true;
}

template <typename... T>
void sink(T...) {}

template <typename indexer_output_t, typename Type, typename BN, std::size_t... Seq>
void check_edge(tbb::flow::graph& g, BN& bn, tbb::flow::buffer_node<indexer_output_t>& buf, Type, tbb::detail::index_sequence<Seq...>) {
    sink(check_edge<indexer_output_t>(g, std::get<Seq>(bn), buf, typename std::tuple_element<Seq, Type>::type(Seq))...);
}

template <typename... Args, std::size_t... Seq>
void test_follows_impl(std::tuple<Args...> t, tbb::detail::index_sequence<Seq...> seq) {
    using namespace tbb::flow;
    using indexer_output_t = typename indexer_node<Args...>::output_type;

    graph g;
    auto bn = std::make_tuple(broadcast_node<Args>(g)...);

    indexer_node<Args...> my_indexer(follows(std::get<Seq>(bn)...));

    buffer_node<indexer_output_t> buf(g);
    make_edge(my_indexer, buf);

    check_edge<indexer_output_t>(g, bn, buf, t, seq);
}

template <typename... Args>
void test_follows() {
    test_follows_impl(std::tuple<Args...>(), tbb::detail::make_index_sequence<sizeof...(Args)>());
}

void test_precedes() {
    using namespace tbb::flow;

    using indexer_output_t = indexer_node<int, float, double>::output_type;

    graph g;

    broadcast_node<int> start1(g);
    broadcast_node<float> start2(g);
    broadcast_node<double> start3(g);

    buffer_node<indexer_output_t> buf1(g);
    buffer_node<indexer_output_t> buf2(g);
    buffer_node<indexer_output_t> buf3(g);

    indexer_node<int, float, double> node(precedes(buf1, buf2, buf3));

    make_edge(start1, input_port<0>(node));
    make_edge(start2, input_port<1>(node));
    make_edge(start3, input_port<2>(node));

    check_edge<indexer_output_t, int>(g, start1, buf1, 1);
    check_edge<indexer_output_t, float>(g, start2, buf2, 2.2f);
    check_edge<indexer_output_t, double>(g, start3, buf3, 3.3);
}

void test_follows_and_precedes_api() {
    test_follows<double>();
    test_follows<int, double>();
    test_follows<int, float, double>();
    test_follows<float, double, int, double>();
    test_follows<float, double, int, double, double>();
    test_follows<float, double, int, double, double, float>();
    test_follows<float, double, int, double, double, float, long>();
    test_follows<float, double, int, double, double, float, long, int>();
    test_follows<float, double, int, double, double, float, long, int, long>();
    test_follows<float, double, int, double, double, float, long, int, float, long>();
    test_precedes();
}
#endif // __TBB_PREVIEW_FLOW_GRAPH_NODE_SET

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
void test_deduction_guides() {
    using namespace tbb::flow;
    graph g;

    broadcast_node<int> b1(g);
    broadcast_node<double> b2(g);
    indexer_node<int, double> i0(g);

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
    indexer_node i1(follows(b1, b2));
    static_assert(std::is_same_v<decltype(i1), indexer_node<int, double>>);
#endif

    indexer_node i2(i0);
    static_assert(std::is_same_v<decltype(i2), indexer_node<int, double>>);
}

#endif

//! Serial and parallel test on various tuple sizes
//! \brief \ref error_guessing
TEST_CASE("Serial and parallel test") {
    INFO("Testing indexer_node, ");

   for (int p = 0; p < 2; ++p) {
       generate_test<serial_test, float>::do_test();
#if MAX_TUPLE_TEST_SIZE >= 4
       generate_test<serial_test, float, double, int, short>::do_test();
#endif
#if MAX_TUPLE_TEST_SIZE >= 6
       generate_test<serial_test, double, double, int, long, int, short>::do_test();
#endif
#if MAX_TUPLE_TEST_SIZE >= 8
       generate_test<serial_test, float, double, double, double, float, int, float, long>::do_test();
#endif
#if MAX_TUPLE_TEST_SIZE >= 10
       generate_test<serial_test, float, double, int, double, double, float, long, int, float, long>::do_test();
#endif
       generate_test<parallel_test, float, double>::do_test();
#if MAX_TUPLE_TEST_SIZE >= 3
       generate_test<parallel_test, float, int, long>::do_test();
#endif
#if MAX_TUPLE_TEST_SIZE >= 5
       generate_test<parallel_test, double, double, int, int, short>::do_test();
#endif
#if MAX_TUPLE_TEST_SIZE >= 7
       generate_test<parallel_test, float, int, double, float, long, float, long>::do_test();
#endif
#if MAX_TUPLE_TEST_SIZE >= 9
       generate_test<parallel_test, float, double, int, double, double, long, int, float, long>::do_test();
#endif
   }
}

#if __TBB_PREVIEW_FLOW_GRAPH_NODE_SET
//! Test follows and precedes API
//! \brief \ref error_guessing
TEST_CASE("Follows and precedes API") {
    test_follows_and_precedes_api();
}
#endif

#if __TBB_CPP17_DEDUCTION_GUIDES_PRESENT
//! Test deduction guides
//! \brief \ref requirement
TEST_CASE("Deduction guides") {
    test_deduction_guides();
}
#endif

