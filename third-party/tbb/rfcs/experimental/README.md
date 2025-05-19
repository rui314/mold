# Design Documents for Experimental Features

Experimental proposals describe extensions that are implemented and
released as preview features in the oneTBB library. A preview
feature is expected to have an implementation that is of comparable quality
to a fully supported feature. Sufficient tests are required.

An experimental feature does not yet appear as part of the oneTBB 
specification. Therefore, the interface and design can change.
There is no commitment to backward compatibility for a preview
feature.

The documents in this directory 
should include a list of the exit conditions that need to be met to move from
preview to fully supported. These conditions might include demonstrated
performance improvements, demonstrated interest from the community,
acceptance of the required oneTBB specification changes, etc. 

For features that require oneTBB specification changes, the document might
include wording for those changes or a link to any PRs that opened
against the specification.

Proposals should not remain in the experimental directory forever.
It should move either to the
supported folder when they become fully supported or the archived 
folder if they are not fully accepted. It should be highly unusual for 
a proposal to stay in the experimental folder for longer than a year or 
two.
