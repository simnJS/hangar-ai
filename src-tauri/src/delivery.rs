//! Deliveries: a worker's branch landing on its workspace's branch.
//!
//! One at a time per repository. Each one is merged onto the branch as it is
//! now in a worktree of its own, checked with the workspace's command, read by
//! a reviewer agent with a fresh context, and only then landed, or handed back
//! to its worker with what failed. Nothing is ever pushed.
