# Contributing to mold

Summary: When you send a patch that is longer than 10 lines, you must
agree that the patch is released under the dual license of the GNU
AGPLv3 and the MIT license. As an indication of agreement, please add
a `Signed-off-by` line to your commit message using `git commit -s`.

Here is the long official version:

mold is currently licensed under the GNU AGPLv3, but I want to relicense
it under a more liberal license in the future. To make this possible,
I want all patches to be submitted under the dual license of the GNU
AGPLv3 and the MIT license (the "Dual License") from the beginning.

To make it clear that contributors have agreed to release their
patches under the Dual License, I introduced a sign-off procedure just
like the Linux kernel project did. The sign-off is a simple line at
the end of the git commit message, which certifies that you wrote it
or otherwise have the right to pass it on as an open-source patch. The
rules are pretty simple: if you can certify the below:

> ## Developer's Certificate of Origin
>
> By making a contribution to this project, I certify that:
>
>  1. The contribution was created in whole or in part by me and I
>     have the right to submit it under the Dual License; or
>
>  2. The contribution is based upon previous work that, to the best
>     of my knowledge, is covered under the the Dual License and I
>     have the right under the Dual License to submit that work with
>     modifications, whether created in whole or in part by me, under
>     the Dual License (unless I am permitted to submit under
>     different licenses), as indicated in the file; or
>
>  3. The contribution was provided directly to me by some other
>     person who certified (1), (2) or (3) and I have not modified it.
>
>  4. I understand and agree that this project and the contribution
>     are public and that a record of the contribution (including all
>     personal information I submit with it, including my sign-off) is
>     maintained indefinitely and may be redistributed consistent with
>     this project or the open source license(s) involved.

then you just add a line saying:

```
Signed-off-by: Random J Developer <random@developer.example.org>
```

using your real name (sorry, no pseudonyms or anonymous contributions.)
This will be done for you automatically if you use ``git commit -s``.
Reverts should also include "Signed-off-by". ``git revert -s`` does that
for you.

We assume that any contribution that's 10 lines or less does not meet
the threshold of originality and therefore copyright does not apply.
You don't need to add a "Signed-off-by" line to such small commits.
