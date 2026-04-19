# oneTBB Design Documents/RFCs

The RFC process intends to:

- Communicate library-wide changes
- Collect feedback before implementation
- Increase transparency in decision-making 
- Align different teams involved in oneTBB development

This directory contains design documents (RFCs) approved 
or rejected for implementation in oneTBB.

The possible RFC states are:

1. Initial 
2. Proposed
3. Experimental
4. Supported
5. Archived

Most modifications or new features will naturally start as a part of a 
GitHub issue or discussion. Small changes do not require a formal RFC. 
However, if the issue or discussion results in an idea for a significant 
change or new feature that affects the library's public API or architecture, 
we recommend opening a PR to add a new RFC to the `rfcs/proposed` directory. 
The RFC should provide a detailed description and design of the proposed feature.
or new feature that significantly impacts the library's public API or 
architecture, it will be suggested that a PR be opened to add a new rfc 
to the `rfcs/proposed` directory. The RFC contains a more detailed description
and design for the feature.

## General Process

A template for RFCs is available as [template.md](template.md). Place the modified
template in the subdirectory of the `rfcs/proposed` with a name
of the form `<feature>_<extension_description>`. For example,
a proposal for a new ``my_op`` flow graph node should be put into the
`rfcs/proposed/flow_graph_my_op_node` directory. Use [template.md](template.md) 
to create the `README.md` file in that directory. The folder can 
contain other files referenced by the `README.md` file, such as figures.

Once two maintainers approve the PR, it is merged into the `rfcs/proposed`
directory. Update the RFC document with additional information as the RFC moves 
to different states. 

A proposal that is subsequently implemented and released in oneTBB 
as a preview feature is moved into the `rfcs/experimental` folder. The
RFC for a preview feature in `rfcs/experimental` should include a description
of what is required to move from experimental to fully supported -- for 
example, feedback from users, demonstrated performance improvements, etc.

A proposal that is implemented, added to the oneTBB specification, and 
supported as a full feature appears in the `rfcs/supported` directory. An RFC 
for a fully supported feature in the `rfcs/supported` directory should 
have a link to the section in the oneTBB specification with its 
formal wording.

A feature that is removed or a proposal that is abandoned or rejected will 
be moved to the `rfcs/archived` folder.

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
