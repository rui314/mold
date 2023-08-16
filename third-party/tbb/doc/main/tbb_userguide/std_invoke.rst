.. _std_invoke:

Invoke a Callable Object
==========================

Starting from C++17, the requirements for callable objects passed to algorithms or Flow Graph nodes are relaxed. It allows using additional types of bodies. 
Previously, the body of the algorithm or Flow Graph node needed to be a Function Object (see `C++ Standard Function Object <https://en.cppreference.com/w/cpp/utility/functional>`_) and provide an 
``operator()`` that accepts input parameters. 

Now the body needs to meet the more relaxed requirements of being Callable (see `C++ Standard Callable <https://en.cppreference.com/w/cpp/named_req/Callable>`_) that covers three types of objects:

* **Function Objects that provide operator(arg1, arg2, ...)**, which accepts the input parameters
* **Pointers to member functions** that you can use as the body of the algorithm or the Flow Graph node
* **Pointers to member objects** work as the body of the algorithm or parallel construct

You can use it not only for a Flow Graph but also for algorithms. See the example below: 

.. code::
   
    // The class models oneTBB Range 
    class StrideRange {
    public:
        StrideRange(int* s, std::size_t sz, std::size_t str)
            : start(s), size(sz), stride(str) {}

        // A copy constructor
        StrideRange(const StrideRange&) = default;

        // A splitting constructor
        StrideRange(StrideRange& other, oneapi::tbb::split) 
            : start(other.start), size(other.size / 2)
        {
            other.size -= size;
            other.start += size;
        }

        ~StrideRange() = default;

        // Indicate if the range is empty
        bool empty() const {
            return size == 0;
        }

        // Indicate if the range can be divided
        bool is_divisible() const {
            return size >= stride;
        }

        void iterate() const {
            for (std::size_t i = 0; i < size; i += stride) {
                // Performed an action for each element of the range,
                // implement the code based on your requirements
            }
        }

    private:
        int* start;
        std::size_t size;
        std::size_t stride;
    };

Where:

* The ``StrideRange`` class models oneTBB range that should be iterated with a specified stride during its initial construction. 
* The ``stride`` value is stored in a private field within the range. Therefore, the class provides the member function ``iterate() const`` that implements a loop with the specified stride. 

``range.iterate()``
*******************

Before C++17, to utilize a range in a parallel algorithm, such as ``parallel_for``, it was required to provide a ``Function Object`` as the algorithm's body. This Function Object defined the operations to be executed on each iteration of the range:

.. code:: 

    int main() {
        std::size_t array_size = 1000;

        int* array_to_iterate = new int[array_size];
        
        StrideRange range(array_to_iterate, array_size, /* stride = */ 2);

        // Define a lambda function as the body of the parallel_for loop
        auto pfor_body = [] (const StrideRange& range) {
            range.iterate();
        };

        // Perform parallel iteration 
        oneapi::tbb::parallel_for(range, pfor_body);

        delete[] array_to_iterate;
    }

An additional lambda function ``pfor_body`` was also required. This lambda function invoked the ``rage.iterate()`` function.

Now with C++17, you can directly utilize a pointer to ``range.iterate()`` as the body of the algorithm:

.. code::
   
    int main() {
        std::size_t array_size = 1000;

        int* array_to_iterate = new int[array_size];

        // Performs the iteration over the array elements with the specified stride
        StrideRange range(array_to_iterate, array_size, /* stride = */ 2);

        // Parallelize the iteration over the range object
        oneapi::tbb::parallel_for(range, &StrideRange::iterate);

        delete[] array_to_iterate;
    }

``std::invoke``
****************

``std::invoke`` is a function template that provides a syntax for invoking different types of callable objects with a set of arguments.

oneTBB implementation uses the C++ standard function ``std::invoke(&StrideRange::iterate, range)`` to execute the body. It is the equivalent of ``range.iterate()``.
Therefore, it allows you to invoke a callable object, such as a function object, with the provided arguments. 

.. tip:: Refer to `C++ Standard <https://en.cppreference.com/w/cpp/utility/functional/invoke>`_ to learn more about ``std::invoke``. 

Example
^^^^^^^^

Consider a specific scenario with ``function_node`` within a Flow Graph.

In the example below, a ``function_node`` takes an object as an input to read a member object of that input and proceed it to the next node in the graph:

.. code:: 

    struct Object {
        int number;
    };

    int main() {
        using namespace oneapi::tbb::flow;

        // Lambda function to read the member object of the input Object
        auto number_reader = [] (const Object& obj) {
            return obj.number;
        };

        // Lambda function to process the received integer
        auto number_processor = [] (int i) { /* processing integer */ };

        graph g;

        // Function node that takes an Object as input and produces an integer
        function_node<Object, int> func1(g, unlimited, number_reader);

        // Function node that takes an integer as input and processes it
        function_node<int, int> func2(g, unlimited, number_processor);

        // Connect the function nodes
        make_edge(func1, func2);

        // Provide produced input to the graph
        func1.try_put(Object{1});

        // Wait for the graph to complete
        g.wait_for_all();
    }


Before C++17, the ``function_node`` in the Flow Graph required the body to be a Function Object. A lambda function was required to extract the number from the Object. 

With C++17, you can use ``std::invoke`` with a pointer to the member number directly as the body. 

You can update the previous example as follows:

.. code::

    struct Object {
        int number;
    };

    int main() {
        using namespace oneapi::tbb::flow;
 
        // The processing logic for the received integer
        auto number_processor = [] (int i) { /* processing integer */ };

        // Create a graph object g to hold the flow graph
        graph g;

        // Use a member function pointer to the number member of the Object struct as the body
        function_node<Object, int> func1(g, unlimited, &Object::number);

        // Use the number_processor lambda function as the body
        function_node<int, int> func2(g, unlimited, number_processor);

        // Connect the function nodes
        make_edge(func1, func2);

        // Connect the function nodes
        func1.try_put(Object{1});

       // Wait for the graph to complete
       g.wait_for_all();
    }

Find More 
*********

The following APIs supports Callable object as Bodies: 

* `parallel_for <https://oneapi-src.github.io/oneAPI-spec/spec/elements/oneTBB/source/algorithms/functions/parallel_for_func.html>`_
* `parallel_reduce <https://oneapi-src.github.io/oneAPI-spec/spec/elements/oneTBB/source/algorithms/functions/parallel_reduce_func.html>`_
* `parallel_deterministic_reduce <https://oneapi-src.github.io/oneAPI-spec/spec/elements/oneTBB/source/algorithms/functions/parallel_deterministic_reduce_func.html>`_
* `parallel_for_each <https://oneapi-src.github.io/oneAPI-spec/spec/elements/oneTBB/source/algorithms/functions/parallel_for_each_func.html>`_
* `parallel_scan <https://oneapi-src.github.io/oneAPI-spec/spec/elements/oneTBB/source/algorithms/functions/parallel_scan_func.html>`_ 
* `parallel_pipeline <https://oneapi-src.github.io/oneAPI-spec/spec/elements/oneTBB/source/algorithms/functions/parallel_pipeline_func.html>`_ 
* `function_node <https://oneapi-src.github.io/oneAPI-spec/spec/elements/oneTBB/source/flow_graph/func_node_cls.html>`_ 
* `multifunction_node <https://oneapi-src.github.io/oneAPI-spec/spec/elements/oneTBB/source/flow_graph/multifunc_node_cls.html>`_ 
* `async_node <https://oneapi-src.github.io/oneAPI-spec/spec/elements/oneTBB/source/flow_graph/async_node_cls.html>`_ 
* `sequencer_node <https://oneapi-src.github.io/oneAPI-spec/spec/elements/oneTBB/source/flow_graph/sequencer_node_cls.html>`_ 
* `join_node with key_matching policy <https://oneapi-src.github.io/oneAPI-spec/spec/elements/oneTBB/source/flow_graph/join_node_cls.html>`_ 
