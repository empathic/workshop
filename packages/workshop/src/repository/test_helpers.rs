use sqlx::sqlite::SqlitePoolOptions;

/// Create a fresh ConversationRepository backed by an in-memory SQLite database.
/// Each call returns an isolated database with all migrations applied (~1ms).
pub async fn test_repository() -> super::ConversationRepository {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("Failed to create in-memory SQLite pool");

    crate::db::run_migrations(&pool)
        .await
        .expect("Failed to run migrations");

    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .expect("Failed to enable foreign keys");

    super::ConversationRepository::new(pool)
}
