use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::app::{
    domain::entity::Identity,
    repo::{AccountRepo, IdentityRepo},
};

/// 身份管理服务
pub struct IdentityService {
    pool: PgPool,
}

impl IdentityService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 创建新身份
    pub async fn create_identity(&self, name: Option<&str>) -> Result<Identity> {
        IdentityRepo::create(&self.pool, name, None).await
    }

    /// 将账号绑定到身份
    pub async fn bind_account(&self, identity_id: Uuid, account_id: i64) -> Result<()> {
        AccountRepo::bind_identity(&self.pool, account_id, identity_id).await?;
        Ok(())
    }
}
