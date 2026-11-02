# CaiRezekiYojana build workflow (Argo Workflows)

Builds the image with **BuildKit** (rootless, daemonless -
`buildctl-daemonless.sh`, no privileged Docker-in-Docker) from the source on
**GitHub**, and pushes it to `docker.io/YOUR-DOCKERHUB-USER/cairezekiyojana`. Nothing local
is mounted: push your work to GitHub before submitting a build.

## Set up once

1. **Point the template at the repository.** In
   `10-build-workflowtemplate.yaml` replace `YOUR-GITHUB-OWNER` in the
   `git-repo` parameter (or pass `-p git-repo=...` on every run). Change the
   `image` parameter (and `deploy/30-deployment.yaml`) to your Docker Hub namespace, replacing `YOUR-DOCKERHUB-USER`.
2. **Fill in the credentials** - `01-secrets.yaml` ships blank, and holds the
   instructions. Two secrets, both in the `invest` namespace:
   - `cairezeki-git-creds` - a GitHub username and access token, only needed
     while the repo is private. Left blank, the clone is anonymous.
   - `cairezeki-dockerhub-creds` - a Docker Hub username and access token
     (Read & Write), to push the image.

   Create them with the `kubectl create secret` commands in that file, so a
   token never touches disk. Do not commit a filled-in copy.
3. **ServiceAccount**, if it is not there already (the nestivesh build uses the
   same one): `kubectl apply -f 00-serviceaccount.yaml`.
4. **Register the template**: `kubectl apply -f 10-build-workflowtemplate.yaml`.

`invest` must be listed in `controller.workflowNamespaces` in the Argo Workflows
values, so the controller watches Workflows submitted there.

## Running a build

```
argo submit -n invest --from workflowtemplate/cairezeki-build --watch
```

A different branch or tag, or a release tag of your own:

```
argo submit -n invest --from workflowtemplate/cairezeki-build \
  -p git-revision=some-branch \
  -p version-tag=2026.11.3 \
  --watch
```

Every run pushes three tags: `<image>:<commit-sha>`, `<image>:<version-tag>`
and `<image>:latest`. The BuildKit layer cache for every stage (`mode=max`) is
pushed to `<image>:cairezeki-compile` and read back on the next run, so the Rust
dependency compile is skipped unless `Cargo.toml`, `Cargo.lock` or the pinned
base image changes. That tag holds cache only - it is not a runnable image.
The first build compiles everything and takes a while.

## Deploying what it built

The workflow only builds and pushes. To roll the cluster over:

```
kubectl -n invest rollout restart deployment/cairezeki
```

(`deploy/30-deployment.yaml` pulls `:latest` with `imagePullPolicy: Always`.)
The first deployment needs `deploy/01-secrets.yaml` filled in - the Supabase
connection - before the pod can start.

## Not set up

- **Auto-trigger on push.** A GitHub webhook into an EventSource + Sensor could
  submit this template on every push instead of `argo submit` by hand.
- **arm64.** This builds for the architecture of the node it runs on.
