---
name: Run clippy before committing
description: Always run `cargo clippy` before creating any git commit to catch lint errors before CI/CD
type: feedback
---

Always run `cargo clippy -- -D warnings` before committing. The CI/CD pipeline enforces clippy with `-D warnings` (deny all warnings), so any clippy issue will fail the build.

**Why:** User has been repeatedly burned by CI failures from clippy warnings that could have been caught locally. This wastes time re-running CI/CD.

**How to apply:** Before every `git commit`, run `cargo clippy -- -D warnings` and fix any issues. Do not skip this step even for "trivial" changes.