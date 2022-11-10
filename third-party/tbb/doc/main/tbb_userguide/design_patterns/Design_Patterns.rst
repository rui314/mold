.. _design_patterns:

Design Patterns
===============


This section provides some common parallel programming patterns and how
to implement them in |full_name|.


The description of each pattern has the following format:


-  **Problem** – describes the problem to be solved.


-  **Context** – describes contexts in which the problem arises.


-  **Forces** - considerations that drive use of the pattern.


-  **Solution** - describes how to implement the pattern.


-  **Example** – presents an example implementation.


Variations and examples are sometimes discussed. The code examples are
intended to emphasize key points and are not full-fledged code. Examples
may omit obvious const overloads of non-const methods.


Much of the nomenclature and examples are adapted from Web pages created
by Eun-Gyu and Marc Snir, and the Berkeley parallel patterns wiki. See
links in the **General References** section.


For brevity, some of the code examples use C++11 lambda expressions. It
is straightforward, albeit sometimes tedious, to translate such lambda
expressions into equivalent C++03 code.

.. toctree::
   :maxdepth: 4

   ../../tbb_userguide/design_patterns/Agglomeration
   ../../tbb_userguide/design_patterns/Elementwise
   ../../tbb_userguide/design_patterns/Odd-Even_Communication
   ../../tbb_userguide/design_patterns/Wavefront
   ../../tbb_userguide/design_patterns/Reduction
   ../../tbb_userguide/design_patterns/Divide_and_Conquer
   ../../tbb_userguide/design_patterns/GUI_Thread
   ../../tbb_userguide/design_patterns/Non-Preemptive_Priorities
   ../../tbb_userguide/design_patterns/Lazy_Initialization
   ../../tbb_userguide/design_patterns/Local_Serializer
   ../../tbb_userguide/design_patterns/Fenced_Data_Transfer
   ../../tbb_userguide/design_patterns/Reference_Counting
   ../../tbb_userguide/design_patterns/General_References
