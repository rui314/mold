#! /usr/bin/env python3

import github
import os
import sys
import time

RETRIES = 10

g = github.Github(os.environ["GITHUB_TOKEN"])
tag_name = os.environ["GITHUB_TAG"]
tag_prefix = "refs/tags/"
if tag_name.startswith(tag_prefix):
    tag_name = tag_name[len(tag_prefix) :]
assert len(sys.argv) == 2
asset_path = sys.argv[1]
asset_name = os.path.basename(asset_path)

repo = g.get_repo(os.environ["GITHUB_REPOSITORY"])

tags = list(repo.get_tags())

for tag in tags:
    if tag.name == tag_name:
        break
else:
    raise RuntimeError("no tag named " + repr(tag_name))

try:
    print("Creating GitHub release for tag " + repr(tag_name) + "...")
    repo.create_git_release(tag_name, tag_name, tag.commit.commit.message)
except github.GithubException as github_error:
    if github_error.data["errors"][0]["code"] == "already_exists":
        print("Release for tag " + repr(tag_name) + " already exists.")
    else:
        raise


def get_release():
    for i in range(RETRIES):
        releases = list(repo.get_releases())
        for release in releases:
            if release.tag_name == tag_name:
                return release
        print(f"Release for tag {repr(tag_name)} not found. Retrying...")
        time.sleep(1)
    raise RuntimeError("no release for tag " + repr(tag_name))


release = get_release()

print("Uploading " + repr(asset_path) + "...")
for i in range(RETRIES):
    try:
        print("Upload attempt #{} of {}...".format(i + 1, RETRIES))
        release.upload_asset(asset_path)
        break
    except github.GithubException as github_error:
        # Unfortunately the asset upload API is flaky. Even worse, it often
        # partially succeeds, returning an error to the caller but leaving the
        # release in a state where subsequent uploads of the same asset will
        # fail with an "already_exists" error. (Though the asset is not visible
        # on github.com, so we can't just declare victory and move on.) If we
        # detect this case, explicitly delete the asset and continue retrying.
        print(github_error)
        for asset in release.get_assets():
            if asset.name == asset_name:
                print("Found uploaded asset after failure. Deleting...")
                asset.delete_asset()
else:
    raise RuntimeError("All upload attempts failed.")

print("Success!")
