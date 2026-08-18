mod files;
mod git;
mod watch;

pub use files::{read_file, tree, write_file};
pub use git::{commit, diff, status};
pub use watch::watch_workspace;
