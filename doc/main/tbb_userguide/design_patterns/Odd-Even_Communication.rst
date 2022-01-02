.. _Odd-Even_Communication:

Odd-Even Communication
======================


.. container:: section


   .. rubric:: Problem
      :class: sectiontitle

   Operations on data cannot be done entirely independently, but data
   can be partitioned into two subsets such that all operations on a
   subset can run in parallel.


.. container:: section


   .. rubric:: Context
      :class: sectiontitle

   Solvers for partial differential equations can often be modified to
   follow this pattern. For example, for a 2D grid with only
   nearest-neighbor communication, it may be possible to treat the grid
   as a checkerboard, and alternate between updating red squares and
   black squares.


   Another context is staggered grid ("leap frog") Finite Difference
   Time Domain (FDTD solvers, which naturally fit the pattern.


.. container:: section


   .. rubric:: Forces
      :class: sectiontitle

   -  Dependencies between items form a bipartite graph.


.. container:: section


   .. rubric:: Solution
      :class: sectiontitle

   Alternate between updating one subset and then the other subset.
   Apply the elementwise pattern to each subset.

.. container:: section


   .. rubric:: References
      :class: sectiontitle

   Eun-Gyu Kim and Mark Snir, "Odd-Even Communication Group",
   http://snir.cs.illinois.edu/patterns/oddeven.pdf

