# Descriptive Name for the Proposal

## Introduction

Short description of the idea proposed with explained motivation. 

The motivation could be:
- Improved users experience for API changes and extensions. Code snippets to
  showcase the benefits would be nice here.
- Performance improvements with the data, if available.
- Improved engineering practices.

Introduction may also include any additional information that sheds light on
the proposal, such as history of the matter, links to relevant issues and
discussions, etc.

## Proposal

A full and detailed description of the proposal with highlighted consequences.

Depending on the kind of the proposal, the description should cover:

- New use cases supported by the extension.
- The expected performance benefit for a modification. 
- The interface of extensions including class definitions or function 
declarations.

A proposal should clearly outline the alternatives that were considered, 
along with their pros and cons. Each alternative should be clearly separated 
to make discussions easier to follow.

Pay close attention to the following aspects of the library:
- API and ABI backward compatibility. The library follows semantic versioning
  so if any of those interfaces are to be broken, the RFC needs to state that
  explicitly.
- Performance implications, as performance is one of the main goals of the library.
- Changes to the build system. While the library's primary building system is
  CMake, there are some frameworks that may build the library directly from the sources.
- Dependencies and support matrix: does the proposal bring any new
  dependencies or affect the supported configurations?

Some other common subsections here are:
- Discussion: some people like to list all the options first (as separate
  subsections), and then have a dedicated section with the discussion.
- List of the proposed API and examples of its usage.
- Testing aspects.
- Short explanation and links to the related sub-proposals, if any. Such
  sub-proposals could be organized as separate standalone RFCs, but this is
  not mandatory. If the change is insignificant or doesn't make any sense
  without the original proposal, you can have it in the RFC.
- Execution plan (next steps), if approved.

## Open Questions

For new proposals (i.e., those in the `rfcs/proposed` directory), list any
open questions.
