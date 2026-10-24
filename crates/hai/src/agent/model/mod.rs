mod catalog;
mod provider;
mod role;

pub(crate) use catalog::ModelCatalog;
pub(crate) use provider::ProviderRegistry;
pub(crate) use role::{ModelRoles, embedding, resolve};
