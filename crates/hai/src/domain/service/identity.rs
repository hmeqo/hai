use crate::{
    domain::{
        model::{Account, Identity},
        vo::{AccountId, IdentityId},
    },
    error::Result,
};

#[derive(Debug)]
pub struct IdentityService {
    db: toasty::Db,
}

impl IdentityService {
    pub fn new(db: toasty::Db) -> Self {
        Self { db }
    }

    pub async fn create_identity(&self, name: Option<&str>) -> Result<Identity> {
        let now = jiff::Timestamp::now();
        toasty::create!(Identity {
            id: uuid::Uuid::now_v7(),
            name: name.map(String::from),
            created_at: now,
            updated_at: now,
        })
        .exec(&mut self.db.clone())
        .await
        .map_err(Into::into)
    }

    pub async fn bind_account(&self, identity_id: IdentityId, account_id: AccountId) -> Result<()> {
        Account::filter_by_id(account_id.0)
            .update()
            .identity_id(Some(identity_id.0))
            .exec(&mut self.db.clone())
            .await?;
        Ok(())
    }
}
