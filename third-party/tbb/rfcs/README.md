# oneTBB Design Documents/RFCs

The RFC process intends to:

- Communicate library-wide changes
- Collect feedback before implementation
- Increase transparency in decision-making 
- Align different teams involved in oneTBB development

This directory contains design documents (RFCs) approved 
or rejected for implementation in oneTBB.

The possible RFC states are:

1. Proposed
2. Experimental
3. Supported
4. Archived

Most modifications or new features will naturally start as a part of a
GitHub issue or discussion. Small changes do not require a formal RFC.
However, if the issue or discussion results in an idea for a significant
change or new feature that affects the library's public API or architecture,
we recommend opening a PR to add a new RFC to the `rfcs/proposed` directory.

The `rfcs/proposed` directory contains RFCs for proposals that are suggested for
implementation. A proposal may initially be merged into the proposed directory
that contains only an extended motivation and an idea for a solution. Later on, it 
may be iteratively refined but eventually describe the overall design and API for
the proposed functionality. 

The `rfcs/experimental` directory contains RFCs for experimental library features.
In addition to the design, these documents describe the criteria for the described
functionality to exit the experimental status.

The `rfcs/supported` directory contains documents for the fully supported features,
both implemented according to the library specification and provided as extensions.

The `rfcs/archived` directory contains rejected proposals and documents for
the former functionality that has been removed.

A subdirectory for an RFC should have a name of the form `<library_feature>_<extension_description>`
and should contain a plain text file `README`, such as a `README.md` file, that either is 
the RFC document or links to other files and Web resources that describe the functionality.
The directory can contain other supporting files such as images or formulas,
as well as sub-proposals / sub-RFCs.

## General Process

You can collect initial feedback on an idea and input for a formal RFC proposal
using a GitHub discussion. You can add the "RFC" label to the discussion 
to indicate the intent.

To create a new RFC document, open a pull request (PR) to add it to the `rfcs/proposed` directory.
A template for new RFCs is available as [template.md](template.md).
Use it to create the `README.md` file in a subdirectory of `rfcs/proposed` named
`<library_feature>_<extension_description>`. For example,
a proposal for a `new my_op flow graph node` should be put into the
`rfcs/proposed/flow_graph_my_op_node` directory. Put other files referenced by the 
`README.md` file, such as figures, into the same directory. The "RFC" label can be 
used to mark PRs containing RFC/design proposals.

The RFC approval process generally follows the guidelines in the [UXL Foundation Operational Procedures](
https://github.com/uxlfoundation/uxl_operational_procedures/blob/release/Process_Documents/Organization_Operational_Process.md#review--approval-process).
Once two or more maintainers approve the PR, it is merged into the main branch.

RFC documents can be developed iteratively at each stage. For example, an initial RFC
can be approved even if some details of the design or the API are not yet sufficiently
elaborated. In that case, subsequent revisions (new PRs) should update the document
in `rfcs/proposed`, adding the requested information.

A proposal that is subsequently implemented and released as an experimental feature
is moved into the `rfcs/experimental` directory.
The RFC for such a feature should include a description
of what is required to move it from experimental to fully supported - for 
example, feedback from users, demonstrated performance improvements, etc.

A proposal that is implemented as a fully supported feature appears
in the `rfcs/supported` directory. It typically involves the oneTBB specification
changes and should therefore have a link to the section in the specification
with its formal wording.

A feature that is removed or a proposal that is abandoned or rejected will 
be moved to the `rfcs/archived` directory. It should state the reasons for
rejection or removal.

There is no requirement that an RFC should pass all the stages in order.
A typical flow for an RFC would include at least `proposed` and `supported`;
however, any state can be skipped, depending on the progress and the needs.

For a document that describes a wide set of functionality or a general direction
and includes sub-RFCs for specific features, a few instances might simultaneously
reside in different states, adjusted as necessary to reflect the overall progress
on the direction and on its sub-proposals.

See the README files in respective directories for additional information.

## Document Style

The design documents are stored in the `rfcs` directory, and each RFC is placed 
in its subdirectory under `rfcs/proposed/<feature>_<extension_description>`. 

- There must be a `README.md` file that contains the main RFC itself (or 
links to a file that contains it in the same directory).
  - The RFC should follow the [template.md](template.md) structure. 
  - The directory can contain other supporting files, such as images, tex 
  formulas, and sub-proposals / sub-RFCs.
  - We highly recommend using a text-based file format like markdown for easy 
  collaboration on GitHub, but other formats like PDFs may also be acceptable.
- For the markdown-written RFC, keep the text width within
  100 characters, unless there is a reason to violate this rule, e.g., 
  long links or wide tables.
- It is also recommended to read through existing RFCs to better understand the 
general writing style and required elements.
