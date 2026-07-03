.. _testing_approach:

Testing Approach 
================

There are four main types of errors/hazards you can encounter in the development of libraries for parallelism:

* Interface correspondence to specification
* Memory errors
* Data race
* Race conditions and deadlocks

|short_name| testing approach is designed to provide high coverage of these error types. 
All types of errors are covered with unit testing and code review.

Code coverage metrics are tracked to ensure high code coverage with tests. Uncovered branches are analyzed manually.
Memory errors and data races are additionally covered by special tools that include thread and memory sanitizers.

Race conditions and deadlocks are the most complicated errors.
They are covered by:

* **Unit tests** that, however, have limited capability to catch such errors.
* **Integration tests**. Multiple different functionalities are heavily combined to emulate user use cases that may trigger such errors based on prior knowledge and expertise. 
* **Stress testing with different possible combinations**. It ensures that even rarely triggered error conditions are caught by testing.

.. note:: Every fix is required to be covered by a regression test to guarantee the detection of such issues in the future.

Continuous Integration triggers all the tests on each commit. This ensures that:

* Issues are detected, starting from the early development phase and up to the moment of integration of changes into the library.
* The highest quality of the library is maintained even in such error-prone domains as parallelism.
