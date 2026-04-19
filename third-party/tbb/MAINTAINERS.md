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

# Introduction

This document defines roles in the oneTBB project.

# Roles and Responsibilities

oneTBB project defines three main roles:
 * [Contributor](#contributor)
 * [Code Owner](#code-Owner)
 * [Maintainer](#maintainer)

[permissions]: https://docs.github.com/en/organizations/managing-user-access-to-your-organizations-repositories/managing-repository-roles/repository-roles-for-an-organization#permissions-for-each-role

|                                                                                                                                             |       Contributor       |       Code Owner        |       Maintainer        |
| :------------------------------------------------------------------------------------------------------------------------------------------ | :---------------------: | :---------------------: | :---------------------: |
| _Responsibilities_                                                                                                                          |                         |                         |                         |
| Follow the [Code of Conduct](./CODE_OF_CONDUCT.md)                                                                                          |            ✓            |            ✓           |            ✓            |
| Follow [Contribution Guidelines](./CONTRIBUTING.md)                                                                                         |            ✓            |            ✓           |            ✓            |
| Ensure [Contribution Guidelines](./CONTRIBUTING.md) are followed                                                                            |            ✗            |            ✓           |            ✓            |
| Co-own component or aspect of the library,<br>  including contributing: bug fixes, implementing features,<br> and performance optimizations |            ✗            |            ✓           |            ✓            |
| Co-own on technical direction of component or<br> aspect of the library including work on RFCs                                              |            ✗            |            ✓           |            ✓            |
| Co-own the project as a whole,<br> including determining strategy and policy for the project                                                |            ✗            |            ✗           |            ✓            |
| _Privileges_                                                                                                                                |                         |                         |                         |
| Permission granted                                                                                                                          |   [Read][permissions]   |   [Write][permissions]  | [Maintain][permissions] |
| Eligible to become                                                                                                                          |       Code Owner        |       Maintainer        |            ✗            |
| Can recommend Contributors<br> to become Code Owner                                                                                         |            ✗            |            ✓           |            ✓            |
| Can participate in promotions of<br> Code Owners and  Maintainers                                                                           |            ✗            |            ✗           |            ✓            |
| Can suggest Milestones during planning                                                                                                      |            ✓            |            ✓           |            ✓            |
| Can choose Milestones for specific component                                                                                                |            ✗            |            ✓           |            ✓            |
| Make a decision on project's Milestones during planning                                                                                     |            ✗            |            ✗           |            ✓            |
| Can propose new RFC or<br> participate in review of existing RFC                                                                            |            ✓            |            ✓           |            ✓            |
| Can request rework of RFCs<br> in represented area of responsibility                                                                        |            ✗            |            ✓           |            ✓            |
| Can request rework of RFCs<br> in any part of the project                                                                                   |            ✗            |            ✗           |            ✓            |
| Can manage release process of the project                                                                                                   |            ✗            |            ✗           |            ✓            |
| Can represent the project in public as a Maintainer                                                                                         |            ✗            |            ✗           |            ✓            |

These roles are merit based. Refer to the corresponding section for specific
requirements and the nomination process.

## Contributor

A Contributor invests time and resources to improve oneTBB project.
Anyone can become a Contributor by bringing value in any following way:
  * Answer questions from community members.
  * Propose changes to the design.
  * Provide feedback on design proposals.
  * Review and/or test pull requests.
  * Test releases and report bugs.
  * Contribute code, including bug fixes, features implementations,
and performance optimizations.

## Code Owner

A Code Owner has responsibility for a specific project component or a functional
area. Code Owners are collectively responsible
for developing and maintaining their component or functional areas, including
reviewing all changes to corresponding areas of responsibility and indicating
whether those changes are ready to be merged. Code Owners have a track record of
contribution and review in the project.

**Requirements:**
  * Track record of accepted code contributions to a specific project component.
  * Track record of contributions to the code review process.
  * Demonstrate in-depth knowledge of the architecture of a specific project
    component.
  * Commit to being responsible for that specific area.

How to become a Code Owner?
1. A Contributor is nominated by opening a PR modifying the MAINTAINERS.md file
including name, Github username, and affiliation.
2. At least two specific component Maintainers approve the PR.
3. [CODEOWNERS](./CODEOWNERS) file is updated to represent corresponding areas of responsibility.

## Maintainer
Maintainers are the most established contributors responsible for the 
project technical direction. They participate in making decisions about the
strategy and priorities of the project.

**Requirements:**
  * Have experience as a Code Owner.
  * Track record of major project contributions to a specific project component.
  * Demonstrate deep knowledge of a specific project component.
  * Demonstrate broad knowledge of the project across multiple areas.
  * Commit to using privileges responsibly for the good of the project.
  * Be able to exercise judgment for the good of the project, independent of
    their employer, friends, or team.

Process of becoming a maintainer:
1. A Maintainer may nominate a current code owner to become a new Maintainer by 
opening a PR against MAINTAINERS.md file.
2. A majority of the current Maintainers must then approve the PR.

# Code Owners and Maintainers List

## oneTBB core (API, Architecture, Tests)

| Name                  | Github ID             | Affiliation       | Role       |
| --------------------- | --------------------- | ----------------- | ---------- |
| Ilya Isaev            | @isaevil              | Intel Corporation | Code Owner |
| Sarath Nandu R        | @sarathnandu          | Intel Corporation | Code Owner |
| Dmitri Mokhov         | @dnmokhov             | Intel Corporation | Code Owner |
| Alexey Kukanov        | @akukanov             | Intel Corporation | Code Owner |
| Konstantin Boyarinov  | @kboyarinov           | Intel Corporation | Maintainer |
| Aleksei Fedotov       | @aleksei-fedotov      | Intel Corporation | Maintainer |
| Michael Voss          | @vossmjp              | Intel Corporation | Maintainer |
| Pavel Kumbrasev       | @pavelkumbrasev       | Intel Corporation | Maintainer |

## oneTBB TBBMALLOC (API, Architecture, Tests)

| Name                  | Github ID             | Affiliation       | Role       |
| --------------------- | --------------------- | ----------------- | ---------- |
| Łukasz Plewa          | @lplewa               | Intel Corporation | Maintainer |


## oneTBB Documentation

| Name                   | Github ID             | Affiliation       | Role       |
| ---------------------- | --------------------- | ----------------- | ---------- |
| Alexandra Epanchinzeva | @aepanchi             | Intel Corporation | Code Owner |


## oneTBB Release Management

| Name               | Github ID             | Affiliation       | Role       |
| ------------------ | --------------------- | ----------------- | ---------- |
| Olga Malysheva     | @omalyshe             | Intel Corporation | Maintainer |

