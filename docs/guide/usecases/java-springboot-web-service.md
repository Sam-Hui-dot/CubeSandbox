---
title: Java Spring Boot Dev/Test Sandbox With Snapshot-Warmed Maven Cache
author: Sam
date: 2026-07-05
tags:
  - java
  - spring-boot
  - backend
  - maven
  - snapshot
lang: en-US
---

# Java Spring Boot Dev/Test Sandbox With Snapshot-Warmed Maven Cache

## Business Context

Enterprise backend teams often need disposable Java environments for API
debugging, contract testing, regression reproduction, and agent-driven code
changes. A useful sandbox must do more than run `java -version`: it should run a
real HTTP service, preserve workspace state, and avoid paying the Maven
dependency warmup cost every time a fresh environment is needed.

The `examples/java-springboot-web` template demonstrates that workflow in
CubeSandbox. The template warms Maven dependencies during a controlled image
build step. The demo then performs an offline build, starts a Spring Boot
service, calls it through CubeSandbox routing, creates task state, snapshots the
workspace, and forks a fresh sandbox that reuses both build artifacts and
service data. Both runtime sandboxes use default-deny public egress.

## Key Challenges

- Java projects can spend meaningful time downloading Maven dependencies before
  the first test can run.
- Backend debugging needs a real HTTP service, not only a language runtime.
- Reproducing a bug or handing work to another agent often requires preserving
  both build artifacts and workspace state.
- Enterprise clusters may restrict direct public Maven downloads.
- The example must stay small enough for reviewers and users to run.

## Solution With CubeSandbox

The template uses digest-pinned CubeSandbox and Maven base images and installs
Java 21, Maven, curl, and bash. The included Spring Boot project exposes four
endpoints:

- `GET /health`
- `GET /api/info`
- `POST /api/tasks`
- `GET /api/tasks/{id}`

The task API persists state to
`/tmp/cubesandbox-spring/state/tasks.json`. The demo script then runs this
sequence:

1. Create a sandbox with `allow_internet_access=False`.
2. Prove direct public HTTPS egress is blocked.
3. Upload the Spring Boot project.
4. Run `mvn --offline -DskipTests package` using the template-warmed
   `/workspace/.m2/repository` and build `target/*.jar`.
5. Start the service and call it through CubeSandbox routing on port `8080`.
6. Create a task and verify the state file exists.
7. Stop the JVM, wait for exit, flush state, and create a checkpoint snapshot.
8. Fork a fresh default-deny sandbox from the checkpoint.
9. Verify that the fork inherited Maven cache, the built jar, and task state.
10. Start Spring Boot directly from the inherited jar and read the original task.
11. Download a manifest proving egress, cache, artifact, state, and routing checks.

This shows CubeSandbox as a reusable JVM backend development environment rather
than a generic Java runtime image.

## Results and Benefits

- Gives Java/Spring Boot users a realistic backend service starting point.
- Demonstrates template-warmed Maven cache plus snapshot-preserved build output
  for dependency-heavy projects.
- Shows stateful workspace inheritance across forked sandboxes.
- Uses normal HTTP service routing through CubeProxy.
- Proves an offline Maven build succeeds while public egress is denied.
- Produces a small JSON manifest that reviewers can inspect after the run.
- Keeps the v1 scope focused by avoiding external databases and multi-container
  orchestration.

## Restricted-Egress Operation

The Dockerfile performs Maven dependency warmup during a controlled template
build. At runtime, the demo proves public HTTPS is blocked under
`allow_internet_access=False` before it completes the Maven offline build. In
production or regulated clusters, the same pattern can be adapted in three ways:

- Prebuild the template image with dependencies already downloaded.
- Configure Maven to use an internal repository mirror.
- Run a controlled network warmup step, preserve the warmed Maven cache in the
  template, and use snapshot/fork as the repeatable test starting point.

After the checkpoint exists, forked sandboxes start from inherited artifacts and
do not need to redownload dependencies for the repeated service run.

## References

- Related example: `examples/java-springboot-web`
- Related issue: [CubeSandbox sandbox templates and example ecosystem](https://github.com/TencentCloud/CubeSandbox/issues/645)
