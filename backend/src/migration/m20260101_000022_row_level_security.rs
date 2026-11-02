//! Supabase serves every table in the `public` schema over its REST API to
//! anyone holding the project's public (anon) key, unless row-level security
//! is on. CaiRezekiYojana never goes through that API - it connects to the
//! database directly, as the owner of these tables, and the owner is not
//! subject to row-level security - so switching it on, with no policies,
//! shuts the REST API out of every table and changes nothing else.
//!
//! Postgres only; SQLite has no such thing.

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DatabaseBackend};

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Every table in `public` - including the ones added later, as long as this
/// is run again - with row-level security on. Idempotent.
const ENABLE: &str = "DO $$ \
    DECLARE t record; \
    BEGIN \
        FOR t IN SELECT tablename FROM pg_tables WHERE schemaname = 'public' LOOP \
            EXECUTE format('ALTER TABLE public.%I ENABLE ROW LEVEL SECURITY', t.tablename); \
        END LOOP; \
    END $$";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let db = m.get_connection();
        if db.get_database_backend() == DatabaseBackend::Postgres {
            db.execute_unprepared(ENABLE).await?;
        }
        Ok(())
    }

    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        // Leaving it on is the safe state.
        Ok(())
    }
}
