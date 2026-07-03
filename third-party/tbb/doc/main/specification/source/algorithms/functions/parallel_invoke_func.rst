.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

===============
parallel_invoke
===============
**[algorithms.parallel_invoke]**

Function template that evaluates several functions in parallel.

.. code:: cpp

    // Defined in header <oneapi/tbb/parallel_invoke.h>

    namespace oneapi {
        namespace tbb {

            template<typename... Functions>
            void parallel_invoke(Functions&&... fs);

        } // namespace tbb
    } // namespace oneapi


Requirements:

* All members of ``Functions`` parameter pack must meet `Function Objects`
  requirements described in the [function.objects] section of the ISO C++ standard.
* Last member of ``Functions`` parameter pack may be a ``task_group_context&`` type.

Evaluates each member passed to ``parallel_invoke`` possibly in parallel. Return values are ignored.

The algorithm can accept a :doc:`task_group_context <../../task_scheduler/scheduling_controls/task_group_context_cls>` object
so that the algorithm’s tasks are executed in this context. By default, the algorithm is executed in a bound context of its own.

Example
-------

The following example evaluates ``f()``, ``g()``, ``h()``, and ``bar(1)`` in parallel.

.. code:: cpp

    #include "oneapi/tbb/parallel_invoke.h"

    extern void f();
    extern void bar(int);

    class MyFunctor {
        int arg;
    public:
        MyFunctor(int a) : arg(a) {}
        void operator()() const { bar(arg); }
    };

    void RunFunctionsInParallel() {
        MyFunctor g(2);
        MyFunctor h(3);

        oneapi::tbb::parallel_invoke(f, g, h, []{bar(1);});
    }
