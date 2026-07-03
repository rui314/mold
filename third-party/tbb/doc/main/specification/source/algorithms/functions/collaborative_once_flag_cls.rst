.. SPDX-FileCopyrightText: 2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

=======================
collaborative_once_flag
=======================
**[algorithms.collaborative_call_once.collaborative_once_flag]**

Special class that ``collaborative_call_once`` uses to perform a call only once.

.. code:: cpp

    // Defined in header <oneapi/tbb/collaborative_call_once.h>

    namespace oneapi {
        namespace tbb {
            
            class collaborative_once_flag {
            public:
                collaborative_once_flag();
                collaborative_once_flag(const collaborative_once_flag&) = delete;
                collaborative_once_flag& operator=(const collaborative_once_flag&) = delete;
            };
        } // namespace tbb
    } // namespace oneapi

Member functions
----------------

.. cpp:function:: collaborative_once_flag()

    Constructs an ``collaborative_once_flag`` object. The initial state indicates that no function has been called.
