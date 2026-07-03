.. SPDX-FileCopyrightText: 2019-2021 Intel Corporation
..
.. SPDX-License-Identifier: CC-BY-4.0

======================
Notational Conventions
======================
**[notational_conventions]**

The following conventions are used in this
document.

==================== ======================================================================================================================================================== ==============================
Convention           Explanation                                                                                                                                              Example
==================== ======================================================================================================================================================== ==============================
*Italic*             Used for introducing new terms, denotation of terms, placeholders, or titles of documents.                                                               The filename consists of the *basename* and the *extension*. For more information, refer to the *TBB Developer Guide*.
-------------------- -------------------------------------------------------------------------------------------------------------------------------------------------------- ------------------------------
``Monospace``        Indicates directory paths and filenames, commands and command line options, function names, methods, classes, data structures in body text, source code. ``oneapi/tbb.h``
                                                                                                                                                                              
                                                                                                                                                                              ``\alt\include``
                                                                                                                                                                              
                                                                                                                                                                              Use the 
                                                                                                                                                                              ``okCreateObjs()`` function to...
                                                                                                                                                                              
                                                                                                                                                                              ``printf("hello, world\n");``
-------------------- -------------------------------------------------------------------------------------------------------------------------------------------------------- ------------------------------
``Monospace italic`` Indicates source code placeholders.                                                                                                                      blocked_range<*Type*>
-------------------- -------------------------------------------------------------------------------------------------------------------------------------------------------- ------------------------------
``Monospace bold``   Emphasizes parts of source code.                                                                                                                         ``x = ( h > 0 ? sizeof(m) : 0xF ) + min;``
-------------------- -------------------------------------------------------------------------------------------------------------------------------------------------------- ------------------------------
[ ]                  Square brackets indicate that the items enclosed in brackets are optional.                                                                                                                 Fa[c]
                                                                                                                                                                              
                                                                                                                                                                              Indicates 
                                                                                                                                                                              ``Fa`` or 
                                                                                                                                                                              ``Fac``.
-------------------- -------------------------------------------------------------------------------------------------------------------------------------------------------- ------------------------------
{ | }                Braces and vertical bars indicate the choice of one item from a selection of two or more items.                                                          X{K | W | P}
                                                                                                                                                                              
                                                                                                                                                                              Indicates 
                                                                                                                                                                              ``XK``, 
                                                                                                                                                                              ``XW``, or 
                                                                                                                                                                              ``XP``.
-------------------- -------------------------------------------------------------------------------------------------------------------------------------------------------- ------------------------------
"[" "]" "{"" }" "|"  Writing a metacharacter in quotation marks negates the syntactical meaning stated above; the character is taken as a literal.                            "[" X "]" [ Y ]
                                                                                                                                                                              
                                                                                                                                                                              Denotes the letter X enclosed in brackets,
                                                                                                                                                                              optionally followed by the letter Y.
-------------------- -------------------------------------------------------------------------------------------------------------------------------------------------------- ------------------------------
...                  The ellipsis indicates that the previous item can be repeated several times.                                                                             **filename** ...
                                                                                                                                                                              
                                                                                                                                                                              Indicates that one or more filenames can be
                                                                                                                                                                              specified.
-------------------- -------------------------------------------------------------------------------------------------------------------------------------------------------- ------------------------------
,...                 The ellipsis preceded by a comma indicates that the previous item can be repeated several times, separated by commas.                                    **word** ,...
                                                                                                                                                                              
                                                                                                                                                                              Indicates that one or more words can be
                                                                                                                                                                              specified. If more than one word is specified, the words are comma-separated.
==================== ======================================================================================================================================================== ==============================

Class members are summarized by informal class
declarations that describe the class as it seems to clients, not how it is
actually implemented. For example, here is an informal declaration of class 
``Foo``:

.. code:: cpp

   class Foo {
   public:
   	int x();
   	int y;
   	~Foo();
   };

The actual implementation might look like:

.. code:: cpp

   namespace internal {
   	class FooBase  {
   	protected:
   		int x();
   	};
   
   	class Foo_v3: protected FooBase {
   	private:
   		int internal_stuff;
   	public:
   		using FooBase::x;
   		int y;
   	};
   }
   
   typedef internal::Foo_v3 Foo;

The example shows two cases where the actual
implementation departs from the informal declaration:

* ``Foo`` is actually a typedef to 
  ``Foo_v3``.
* Method 
  ``x()`` is inherited from a protected base class.
* The destructor is an implicit method generated
  by the compiler.

The informal declarations are intended to show you
what you need to know to use the class without the distraction of irrelevant
clutter particular to the implementation.
