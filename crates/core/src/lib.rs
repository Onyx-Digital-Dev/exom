//! Exom Core Library
//!
//! Core models, permissions, hosting logic, and storage for the Exom platform.

pub mod chest;
pub mod error;
pub mod hosting;
pub mod invariants;
pub mod models;
pub mod permissions;
pub mod storage;

pub use chest::HallChest;
pub use error::{Error, Result};
pub use hosting::*;
pub use models::*;
pub use permissions::*;
pub use storage::{
    Database, HallRepository, InviteRepository, LastConnection, MessageRepository, Storage,
    UserRepository,
};
