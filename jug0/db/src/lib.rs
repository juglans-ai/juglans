pub use sea_orm_migration::prelude::*;

mod m20240101_000001_init;
mod m20240102_000001_seed;
mod m20240103_000001_agent_endpoint;
mod m20240104_000001_workflow_runs;
mod m20240105_000001_user_quotas;
mod m20240106_000001_deploys;
mod m20240107_000001_drop_tier;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20240101_000001_init::Migration),
            Box::new(m20240102_000001_seed::Migration),
            Box::new(m20240103_000001_agent_endpoint::Migration),
            Box::new(m20240104_000001_workflow_runs::Migration),
            Box::new(m20240105_000001_user_quotas::Migration),
            Box::new(m20240106_000001_deploys::Migration),
            Box::new(m20240107_000001_drop_tier::Migration),
        ]
    }
}
