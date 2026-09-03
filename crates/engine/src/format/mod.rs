//! The `.apireq`/`.apicol`/`.apienv` file format: a custom lightweight
//! labeled-block format (see `api-client-spec.md` section 4). `.apicol`
//! (collection manifest) and `.apienv` (environment) typed layers land
//! alongside `apireq` as those parts of the app get built; `blocks` is
//! shared by all three.

pub mod apicol;
pub mod apienv;
pub mod apireq;
pub mod auth;
pub mod blocks;
pub mod vault;

pub use apicol::{CollectionFile, CollectionMeta};
pub use apienv::EnvironmentFile;
pub use apireq::{ApiRequestFile, Assertion, Body, Meta};
pub use auth::{ApiKeyPlacement, Auth, OAuth2Config};
pub use blocks::RawBlock;
pub use vault::VaultFile;
