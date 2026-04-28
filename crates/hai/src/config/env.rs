use crate::config::meta::PROJECT_NAME_UPPER;

pub const ENV_PREFIX: &str = PROJECT_NAME_UPPER;

pub fn local_mode() -> bool {
    std::env::var(format!("{}_LOCAL_MODE", ENV_PREFIX)).is_ok()
}
