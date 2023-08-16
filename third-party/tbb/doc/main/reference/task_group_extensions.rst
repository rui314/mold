.. _task_group_extensions:

task_group extensions
=====================

.. note::
    To enable these extensions, set the ``TBB_PREVIEW_TASK_GROUP_EXTENSIONS`` macro to 1.

.. contents::
    :local:
    :depth: 1

Description
***********

|full_name| implementation extends the `tbb::task_group specification <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/task_group/task_group_cls.html>`_ with the requirements for a user-provided function object.
   

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/task_group.h>

Synopsis
--------

.. code:: cpp

    namespace oneapi {
        namespace tbb {
   
           class task_group {
           public:

               //only the requirements for the return type of function F are changed              
               template<typename F>
               task_handle defer(F&& f);
                   
               //only the requirements for the return type of function F are changed
               template<typename F>
               task_group_status run_and_wait(const F& f);
    
               //only the requirements for the return type of function F are changed              
               template<typename F>
               void run(F&& f);
           }; 

        } // namespace tbb
    } // namespace oneapi



Member Functions
----------------

.. cpp:function:: template<typename F> task_handle  defer(F&& f)

As an optimization hint, ``F`` might return a ``task_handle``, which task object can be executed next.

.. note::
   The ``task_handle`` returned by the function must be created using ``*this`` ``task_group``. That is, the one for which the run method is called, otherwise it is undefined behavior. 

.. cpp:function:: template<typename F> task_group_status run_and_wait(const F& f)

As an optimization hint, ``F`` might return a ``task_handle``, which task object can be executed next.

.. note::
   The ``task_handle`` returned by the function must be created using ``*this`` ``task_group``. That is, the one for which the run method is called, otherwise it is undefined behavior. 

 
.. cpp:function:: template<typename F> void  run(F&& f)

As an optimization hint, ``F`` might return a ``task_handle``, which task object can be executed next.

.. note::
   The ``task_handle`` returned by the function must be created with ``*this`` ``task_group``. It means, with the one for which run method is called, otherwise it is an undefined behavior. 
    
               
.. rubric:: See also

* `oneapi::tbb::task_group specification <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/task_group/task_group_cls.html>`_
* `oneapi::tbb::task_group_context specification <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/scheduling_controls/task_group_context_cls.html>`_
* `oneapi::tbb::task_group_status specification <https://spec.oneapi.com/versions/latest/elements/oneTBB/source/task_scheduler/task_group/task_group_status_enum.html>`_ 
* `oneapi::tbb::task_handle class <https://oneapi-src.github.io/oneAPI-spec/spec/elements/oneTBB/source/task_scheduler/task_group/task_handle.html>`_
