<!--
******************************************************************************
* 
* Licensed under the Apache License, Version 2.0 (the "License");
* you may not use this file except in compliance with the License.
* You may obtain a copy of the License at
*
*     http://www.apache.org/licenses/LICENSE-2.0
*
* Unless required by applicable law or agreed to in writing, software
* distributed under the License is distributed on an "AS IS" BASIS,
* WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
* See the License for the specific language governing permissions and
* limitations under the License.
*******************************************************************************/-->

# How to Contribute
As an open source project, we welcome community contributions to oneAPI Threading Building Blocks (oneTBB).  This document explains how to participate in project conversations, log bugs and enhancement requests, and submit code patches to the project. 

## Licensing 

Licensing is very important to open source projects. It helps ensure the software continues to be available under the terms that the author desired. The oneTBB project uses the [Apache 2.0 License](https://github.com/uxlfoundation/oneTBB/blob/master/LICENSE.txt), a permissive open source license that allows you to freely use, modify, and distribute your own products that include Apache 2.0 licensed software. By contributing to the oneTBB project, you agree to the license and copyright terms therein and release your own contributions under these terms. 

Some imported or reused components within oneTBB use other licenses, as described in [third-party-programs.txt](https://github.com/uxlfoundation/oneTBB/blob/master/third-party-programs.txt). By carefully reviewing potential contributions, we can ensure that the community can develop products with oneTBB without concerns over patent or copyright issues. 

## Prerequisites 

As a contributor, you'll want to be familiar with the oneTBB project and the repository layout. You should also know how to use it as explained in the [oneTBB documentation](https://uxlfoundation.github.io/oneTBB/) and how to set up your build development environment to configure, build, and test oneTBB as explained in the [oneTBB Build System Description](cmake/README.md). 

## Pull Requests 

You can find all [open oneTBB pull requests](https://github.com/uxlfoundation/oneTBB/pulls) on GitHub. 
 
### Before contributing changes directly to the oneTBB repository

* Make sure you can build the product and run all the tests with your patch. 
* For a larger feature, provide a relevant test. 
* Document your code. The oneTBB project uses reStructuredText for documentation.  
* Update the copyright year in the first line of the changing file(s). 
  For example, if you commit your changes in 2022:
  * the copyright year should be `2005-2022` for existing files
  * the copyright year should be `2022` for new files
* Submit a pull request into the master branch. You can submit changes with a pull request (preferred) or by sending an email patch.  

Continuous Integration (CI) testing is enabled for the repository. Your pull request must pass all checks before it can be merged. We will review your contribution and may provide feedback to guide you if any additional fixes or modifications are necessary. When reviewed and accepted, your pull request will be merged into our GitHub repository. 
