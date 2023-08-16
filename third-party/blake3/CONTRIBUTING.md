# Contributing

We welcome and encourage third-party contributions to BLAKE3, be it reports of issues encountered while using the software or proposals of patches.

## Bug reports

Bugs and other problems should be reported on [GitHub Issues](https://github.com/BLAKE3/BLAKE3/issues).

If you report a bug, please:

* Check that it's not already reported in the [GitHub Issues](https://github.com/BLAKE3/BLAKE3/issues).
* Provide information to help us diagnose and ideally reproduce the bug.

## Patches

We encourage you to fix a bug via a [GitHub Pull request](https://github.com/BLAKE3/BLAKE3/pulls), preferably after creating a related issue and referring it in the PR.

If you contribute code and submit a patch, please note the following:

* We use Rust's stable branch for developing BLAKE3.
* Pull requests should target the `master` branch.
* Try to follow the established Rust [style guidelines](https://doc.rust-lang.org/1.0.0/style/).

Also please make sure to create new unit tests covering your code additions. You can execute the tests by running:

```bash
cargo test
```

All third-party contributions will be recognized in the list of contributors.

