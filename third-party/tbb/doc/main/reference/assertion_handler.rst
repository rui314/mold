.. _assertion_handler:

Custom Assertion Handler
========================

.. note::
    The availability of the extension can be checked with the ``TBB_EXT_CUSTOM_ASSERTION_HANDLER`` macro defined in
    ``oneapi/tbb/global_control.h`` and ``oneapi/tbb/version.h`` headers.

.. contents::
    :local:
    :depth: 2

Description
***********

oneTBB implements assertion checking that detects errors in header files and library code. While most assertions are
active in debug builds (controlled by ``TBB_USE_ASSERT``), some assertions remain present in release builds. By
default, failed assertions display an error message and terminate the application. The custom assertion handler
mechanism extends this by allowing developers to implement their own assertion handling functions. The API is
semantically similar to the standard ``std::set_terminate`` and ``std::get_terminate`` functions.

API
***

Header
------

.. code:: cpp

    #include <oneapi/tbb/global_control.h>

Synopsis
--------

.. code:: cpp

    #define TBB_EXT_CUSTOM_ASSERTION_HANDLER 202510

    namespace oneapi {
    namespace tbb {
    namespace ext {
        using assertion_handler_type = void(*)(const char* location, int line,
                                               const char* expression, const char* comment);

        assertion_handler_type set_assertion_handler(assertion_handler_type new_handler) noexcept;

        assertion_handler_type get_assertion_handler() noexcept;
    }}}

Types
-----

.. cpp:type:: assertion_handler_type

Type alias for the pointer to an assertion handler function.

Functions
---------

.. cpp:function:: assertion_handler_type set_assertion_handler(assertion_handler_type new_handler) noexcept

Sets the provided assertion handler and returns the previous handler. If ``new_handler`` is ``nullptr``, resets to the
default handler.

.. note:: ``new_handler`` must not return. If it does, the behavior is undefined.

.. cpp:function:: assertion_handler_type get_assertion_handler() noexcept

Returns the current assertion handler.

Example
*******

.. literalinclude:: ./examples/assertion_handler.cpp
    :language: c++
    :start-after: /*begin_assertion_handler_example*/
    :end-before: /*end_assertion_handler_example*/

.. rubric:: See also

* :onetbb-spec:`Enabling Debugging Features specification <configuration/enabling_debugging_features>`
