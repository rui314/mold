.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

================
tick_count class
================
**[timing.tick_count]**

A ``tick_count`` is an absolute wall clock timestamp. Two ``tick_count``
objects can be subtracted to compute wall clock
duration ``tick_count::interval_t``, which can be converted to seconds.

.. code:: cpp

   namespace oneapi {
   namespace tbb {

       class tick_count {
       public:
           class interval_t;
           tick_count();
           tick_count( const tick_count& );
           ~tick_count();
           tick_count& operator=( const tick_count& );
           static tick_count now();
           static double resolution();
       };

   } // namespace tbb
   } // namespace oneapi

``tick_count()``
  Constructs ``tick_count`` with an unspecified wall clock timestamp.

``tick_count( const tick_count& )``
  Constructs ``tick_count`` with the timestamp of the given ``tick_count``.

``~tick_count()``
  Destructor.

``tick_count& operator=( const tick_count& )``
  Assigns the timestamp of one ``tick_count`` to another.

``static tick_count now()``
  Returns a ``tick_count`` object that represents the current wall clock timestamp.

``static double resolution()``
  Returns the resolution of the clock used by ``tick_count``, in seconds.


============================
tick_count::interval_t class
============================
**[timing.tick_count.interval_t]**

A ``tick_count::interval_t`` represents wall clock duration.

.. code:: cpp

   namespace oneapi {
   namespace tbb {

       class tick_count::interval_t {
       public:
           interval_t();
           explicit interval_t( double );
           ~interval_t();
           interval_t& operator=( const interval_t& );
           interval_t& operator+=( const interval_t& );
           interval_t& operator-=( const interval_t& );
           double seconds() const;
       };

   } // namespace tbb
   } // namespace oneapi


``interval_t()``
  Constructs ``interval_t`` representing zero time duration.

``explicit interval_t( double )``
  Constructs ``interval_t`` representing the specified number of seconds.

``~interval_t()``
  Destructor.

``interval_t& operator=( const interval_t& )``
  Assigns the wall clock duration of one ``interval_t`` to another.

``interval_t& operator+=( const interval_t& )``
  Increases the duration to the given ``interval_t``, and returns ``*this``.

``interval_t& operator-=( const interval_t& )``
  Decreases the duration to the given ``interval_t``, and returns ``*this``.

``double seconds() const``
  Returns the duration measured in seconds.


====================
Non-member functions
====================
**[timing.tick_count.nonmember]**

These functions provide arithmetic binary operations with wall clock timestamps and durations.

.. code:: cpp

   oneapi::tbb::tick_count::interval_t operator-( const oneapi::tbb::tick_count&, const oneapi::tbb::tick_count& );
   oneapi::tbb::tick_count::interval_t operator+( const oneapi::tbb::tick_count::interval_t&, const oneapi::tbb::tick_count::interval_t& );
   oneapi::tbb::tick_count::interval_t operator-( const oneapi::tbb::tick_count::interval_t&, const oneapi::tbb::tick_count::interval_t& );

The namespace where these functions are defined is unspecified as long as they may be used in respective binary operation expressions on ``tick_count`` and ``tick_count::interval_t`` objects. 
For example, an implementation may define the classes and functions in the same unspecified internal namespace, 
and define  ``oneapi::tbb::tick_count`` as a type alias for which the non-member functions are reachable only via argument-dependent lookup. 


``oneapi::tbb::tick_count::interval_t operator-( const oneapi::tbb::tick_count&, const oneapi::tbb::tick_count& )``
  Returns ``interval_t`` representing the duration between two given wall clock timestamps.

``oneapi::tbb::tick_count::interval_t operator+( const oneapi::tbb::tick_count::interval_t&, const oneapi::tbb::tick_count::interval_t& )``
  Returns ``interval_t`` representing the sum of two given intervals.

``oneapi::tbb::tick_count::interval_t operator-( const oneapi::tbb::tick_count::interval_t&, const oneapi::tbb::tick_count::interval_t& )``
  Returns ``interval_t`` representing the difference of two given intervals.
  
