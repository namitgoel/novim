//! Novim Remote — SSH-based remote development.
//!
//! Provides a thin client (local) and headless server (remote)
//! for editing files on remote machines via SSH.
//!
//! Usage:
//!   novim ssh user@host --path /project    # local client
//!   novim serve --path /project            # remote server (launched by SSH)

pub mod protocol;
pub mod transport;
pub mod server;
pub mod client;
