.. _Lambda_Expressions:

Lambda Expressions
==================


C++11 lambda expressions make the |full_name|
``parallel_for`` much easier to use. A lambda expression lets
the compiler do the tedious work of creating a function object.


Below is the example from the previous section, rewritten with a lambda
expression. The lambda expression, replaces both the declaration and construction of function object ``ApplyFoo`` in the
example of the previous section.


.. literalinclude:: ./examples/parallel_for_lambda_example_1.cpp
    :language: c++
    :start-after: /*begin_parallel_for_lambda_1*/
    :end-before: /*end_parallel_for_lambda_1*/

The [=] introduces the lambda expression. The expression creates a
function object very similar to ``ApplyFoo``. When local variables like
``a`` and ``n`` are declared outside the lambda expression, but used
inside it, they are "captured" as fields inside the function object. The
[=] specifies that capture is by value. Writing [&] instead would
capture the values by reference. After the [=] is the parameter list and
definition for the ``operator()`` of the generated function object. The
compiler documentation says more about lambda expressions and other
implemented C++11 features. It is worth reading more complete
descriptions of lambda expressions than can fit here, because lambda
expressions are a powerful feature for using template libraries in
general.


C++11 support is off by default in the compiler. The following table
shows the option for turning it on.


.. container:: tablenoborder


   .. list-table::
      :header-rows: 1

      * -     Environment
        -     Intel® C++ Compiler Classic
        -     Intel® oneAPI DPC++/C++ Compiler
      * -     Windows\* OS systems
        -     \ ``icl /Qstd=c++11 foo.cpp``
        -     \ ``icx /Qstd=c++11 foo.cpp``
      * -     Linux\* OS systems
        -     \ ``icc -std=c++11 foo.cpp``
        -     \ ``icx -std=c++11 foo.cpp``




For further compactness, oneTBB has a form of ``parallel_for`` expressly
for parallel looping over a consecutive range of integers. The
expression ``parallel_for(first,last,step,f)`` is like writing
``for(auto i=first; i<last; i+=step)f(i)`` except that
each f(i) can be evaluated in parallel if resources permit. The ``step``
parameter is optional. Here is the previous example rewritten in the
compact form:

.. literalinclude:: ./examples/parallel_for_lambda_example_2.cpp
    :language: c++
    :start-after: /*begin_parallel_for_lambda_2*/
    :end-before: /*end_parallel_for_lambda_2*/

The compact form supports only unidimensional iteration spaces of
integers and the automatic chunking feature detailed on the following
section.
