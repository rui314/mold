name: Build tarballs

on:
  push:
    branches: [ main ]

jobs:
  build-and-push:
    runs-on: ubuntu-latest
    steps:
    - name: Checkout Repository
      uses: actions/checkout@v2

    - name: Login to GitHub Container Registry
      uses: docker/login-action@v1
      with:
        registry: ghcr.io
        username: ${{ github.repository_owner }}
        password: ${{ secrets.GITHUB_TOKEN }}

    - name: Build a tarball
      run: |
        arch=$(uname -m)
        [[ $arch = arm* ]] && arch=arm
        remote=ghcr.io/${{ github.repository_owner }}/mold-builder-$arch
        local=mold-builder-$arch
        ( docker pull $remote && docker tag $remote $local ) || true
        ./dist.sh
        docker tag $local $remote
        docker push $remote

    - name: Upload artifact
      uses: actions/upload-artifact@v2
      with:
        name: distribution-files
        path: mold-*.tar.gz
