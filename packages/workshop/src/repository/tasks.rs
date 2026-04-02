use anyhow::{Context, Result};
use sqlx::Row;
use tracing::info;

use crate::models::{
    MigrateTaskItem, Tag, Task, TaskDispatch, TaskListFilters, TaskWithTags, UpdateTaskRequest,
};

use super::ConversationRepository;

impl ConversationRepository {
    pub async fn create_task(&self, task: &Task) -> Result<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO tasks (uuid, title, body, status, priority, instance_id, creator_id, creator_name, sort_order, created_at, updated_at, sent_text, conversation_id)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&task.uuid)
        .bind(&task.title)
        .bind(&task.body)
        .bind(&task.status)
        .bind(task.priority)
        .bind(&task.instance_id)
        .bind(&task.creator_id)
        .bind(&task.creator_name)
        .bind(task.sort_order)
        .bind(task.created_at)
        .bind(task.updated_at)
        .bind(&task.sent_text)
        .bind(&task.conversation_id)
        .execute(&self.pool)
        .await
        .context("Failed to create task")?;

        Ok(result.last_insert_rowid())
    }

    pub async fn get_task(&self, id: i64) -> Result<Option<Task>> {
        let row = sqlx::query(
            r#"
            SELECT id, uuid, title, body, status, priority, instance_id, creator_id,
                   creator_name, sort_order, created_at, updated_at, completed_at, is_deleted,
                   sent_text, conversation_id
            FROM tasks
            WHERE id = ? AND is_deleted = 0
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| Task {
            id: r.get("id"),
            uuid: r.get("uuid"),
            title: r.get("title"),
            body: r.get("body"),
            status: r.get("status"),
            priority: r.get("priority"),
            instance_id: r.get("instance_id"),
            creator_id: r.get("creator_id"),
            creator_name: r.get("creator_name"),
            sort_order: r.get("sort_order"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
            completed_at: r.get("completed_at"),
            is_deleted: r.get::<i32, _>("is_deleted") != 0,
            sent_text: r.get("sent_text"),
            conversation_id: r.get("conversation_id"),
        }))
    }

    /// Fetch a single task with its tags and dispatches.
    pub async fn get_task_with_tags(&self, id: i64) -> Result<Option<TaskWithTags>> {
        let task = match self.get_task(id).await? {
            Some(t) => t,
            None => return Ok(None),
        };

        let tag_rows = sqlx::query(
            "SELECT tg.id, tg.name, tg.color FROM tags tg JOIN task_tags tt ON tg.id = tt.tag_id WHERE tt.task_id = ?",
        )
        .bind(id)
        .fetch_all(&self.pool)
        .await?;

        let tags = tag_rows
            .into_iter()
            .map(|r| Tag {
                id: r.get("id"),
                name: r.get("name"),
                color: r.get("color"),
            })
            .collect();

        let dispatches = self
            .get_dispatches_for_tasks(&[id])
            .await
            .unwrap_or_default();

        Ok(Some(TaskWithTags {
            task,
            tags,
            dispatches,
        }))
    }

    pub async fn list_tasks(&self, filters: &TaskListFilters) -> Result<Vec<TaskWithTags>> {
        let mut conditions = vec!["t.is_deleted = 0".to_string()];

        // Build dynamic WHERE clause
        if let Some(ref status) = filters.status {
            conditions.push(format!("t.status = '{}'", status.replace('\'', "''")));
        }
        if let Some(ref instance_id) = filters.instance_id {
            conditions.push(format!(
                "t.instance_id = '{}'",
                instance_id.replace('\'', "''")
            ));
        }
        if let Some(ref search) = filters.search {
            conditions.push(format!(
                "(t.title LIKE '%{}%' OR t.body LIKE '%{}%')",
                search.replace('\'', "''"),
                search.replace('\'', "''")
            ));
        }

        let tag_join = if let Some(ref tag) = filters.tag {
            conditions.push(format!("tg.name = '{}'", tag.replace('\'', "''")));
            "JOIN task_tags tt ON t.id = tt.task_id JOIN tags tg ON tt.tag_id = tg.id"
        } else {
            ""
        };

        let limit = filters.limit.unwrap_or(100);
        let offset = filters.offset.unwrap_or(0);

        let where_clause = conditions.join(" AND ");
        let sql = format!(
            r#"
            SELECT t.id, t.uuid, t.title, t.body, t.status, t.priority, t.instance_id,
                   t.creator_id, t.creator_name, t.sort_order, t.created_at, t.updated_at,
                   t.completed_at, t.is_deleted, t.sent_text, t.conversation_id
            FROM tasks t
            {}
            WHERE {}
            ORDER BY t.sort_order ASC, t.created_at ASC
            LIMIT {} OFFSET {}
            "#,
            tag_join, where_clause, limit, offset
        );

        let rows = sqlx::query(&sql).fetch_all(&self.pool).await?;

        let mut results = Vec::new();
        for r in rows {
            let task_id: i64 = r.get("id");
            let task = Task {
                id: Some(task_id),
                uuid: r.get("uuid"),
                title: r.get("title"),
                body: r.get("body"),
                status: r.get("status"),
                priority: r.get("priority"),
                instance_id: r.get("instance_id"),
                creator_id: r.get("creator_id"),
                creator_name: r.get("creator_name"),
                sort_order: r.get("sort_order"),
                created_at: r.get("created_at"),
                updated_at: r.get("updated_at"),
                completed_at: r.get("completed_at"),
                is_deleted: r.get::<i32, _>("is_deleted") != 0,
                sent_text: r.get("sent_text"),
                conversation_id: r.get("conversation_id"),
            };

            // Fetch tags for this task
            let tag_rows = sqlx::query(
                r#"
                SELECT tg.id, tg.name, tg.color
                FROM tags tg
                JOIN task_tags tt ON tg.id = tt.tag_id
                WHERE tt.task_id = ?
                "#,
            )
            .bind(task_id)
            .fetch_all(&self.pool)
            .await?;

            let tags = tag_rows
                .into_iter()
                .map(|tr| Tag {
                    id: tr.get("id"),
                    name: tr.get("name"),
                    color: tr.get("color"),
                })
                .collect();

            results.push(TaskWithTags {
                task,
                tags,
                dispatches: vec![],
            });
        }

        // Batch-load dispatches for all tasks
        let task_ids: Vec<i64> = results.iter().filter_map(|t| t.task.id).collect();
        if !task_ids.is_empty() {
            let dispatches = self.get_dispatches_for_tasks(&task_ids).await?;
            for twt in &mut results {
                if let Some(tid) = twt.task.id {
                    twt.dispatches = dispatches
                        .iter()
                        .filter(|d| d.task_id == tid)
                        .cloned()
                        .collect();
                }
            }
        }

        Ok(results)
    }

    pub async fn create_task_dispatch(
        &self,
        task_id: i64,
        instance_id: &str,
        sent_text: &str,
    ) -> Result<TaskDispatch> {
        let now = chrono::Utc::now().timestamp();
        let result = sqlx::query(
            r#"
            INSERT INTO task_dispatches (task_id, instance_id, sent_text, sent_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(task_id)
        .bind(instance_id)
        .bind(sent_text)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("Failed to create task dispatch")?;

        Ok(TaskDispatch {
            id: Some(result.last_insert_rowid()),
            task_id,
            instance_id: instance_id.to_string(),
            sent_text: sent_text.to_string(),
            conversation_id: None,
            sent_at: now,
        })
    }

    pub async fn get_dispatches_for_tasks(&self, task_ids: &[i64]) -> Result<Vec<TaskDispatch>> {
        if task_ids.is_empty() {
            return Ok(vec![]);
        }

        let placeholders: Vec<&str> = task_ids.iter().map(|_| "?").collect();
        let query = format!(
            "SELECT id, task_id, instance_id, sent_text, conversation_id, sent_at FROM task_dispatches WHERE task_id IN ({}) ORDER BY sent_at ASC",
            placeholders.join(", ")
        );

        let mut q = sqlx::query(&query);
        for id in task_ids {
            q = q.bind(id);
        }

        let rows = q.fetch_all(&self.pool).await?;

        Ok(rows
            .into_iter()
            .map(|r| TaskDispatch {
                id: r.get("id"),
                task_id: r.get("task_id"),
                instance_id: r.get("instance_id"),
                sent_text: r.get("sent_text"),
                conversation_id: r.get("conversation_id"),
                sent_at: r.get("sent_at"),
            })
            .collect())
    }

    pub async fn update_task(&self, id: i64, update: &UpdateTaskRequest) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let task = self.get_task(id).await?.context("Task not found")?;
        let new_title = update.title.as_deref().unwrap_or(&task.title);
        let new_body = update.body.as_ref().or(task.body.as_ref());
        let new_status = update.status.as_deref().unwrap_or(&task.status);
        let new_priority = update.priority.unwrap_or(task.priority);
        let new_instance_id = if update.instance_id.is_some() {
            update.instance_id.as_deref()
        } else {
            task.instance_id.as_deref()
        };
        let new_sort_order = update.sort_order.unwrap_or(task.sort_order);
        let completed_at = if new_status == "completed" && task.status != "completed" {
            Some(now)
        } else {
            task.completed_at
        };
        let new_sent_text = update.sent_text.as_ref().or(task.sent_text.as_ref());
        let new_conversation_id = update
            .conversation_id
            .as_ref()
            .or(task.conversation_id.as_ref());

        sqlx::query(
            r#"
            UPDATE tasks SET title = ?, body = ?, status = ?, priority = ?,
                   instance_id = ?, sort_order = ?, updated_at = ?, completed_at = ?,
                   sent_text = ?, conversation_id = ?
            WHERE id = ? AND is_deleted = 0
            "#,
        )
        .bind(new_title)
        .bind(new_body)
        .bind(new_status)
        .bind(new_priority)
        .bind(new_instance_id)
        .bind(new_sort_order)
        .bind(now)
        .bind(completed_at)
        .bind(new_sent_text)
        .bind(new_conversation_id)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("Failed to update task")?;

        Ok(())
    }

    pub async fn delete_task(&self, id: i64) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query("UPDATE tasks SET is_deleted = 1, updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("Failed to delete task")?;
        Ok(())
    }

    pub async fn add_task_tag(&self, task_id: i64, tag_name: &str) -> Result<()> {
        // Get or create the tag
        sqlx::query("INSERT OR IGNORE INTO tags (name) VALUES (?)")
            .bind(tag_name)
            .execute(&self.pool)
            .await?;

        let row = sqlx::query("SELECT id FROM tags WHERE name = ?")
            .bind(tag_name)
            .fetch_one(&self.pool)
            .await?;

        let tag_id: i64 = row.get("id");

        sqlx::query("INSERT OR IGNORE INTO task_tags (task_id, tag_id) VALUES (?, ?)")
            .bind(task_id)
            .bind(tag_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn remove_task_tag(&self, task_id: i64, tag_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM task_tags WHERE task_id = ? AND tag_id = ?")
            .bind(task_id)
            .bind(tag_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_next_sort_order(&self, instance_id: Option<&str>) -> Result<f64> {
        let row = if let Some(iid) = instance_id {
            sqlx::query(
                "SELECT COALESCE(MAX(sort_order), 0.0) as max_order FROM tasks WHERE instance_id = ? AND is_deleted = 0",
            )
            .bind(iid)
            .fetch_one(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT COALESCE(MAX(sort_order), 0.0) as max_order FROM tasks WHERE is_deleted = 0",
            )
            .fetch_one(&self.pool)
            .await?
        };
        let max: f64 = row.get("max_order");
        Ok(max + 1.0)
    }

    pub async fn migrate_tasks(&self, items: &[MigrateTaskItem]) -> Result<Vec<i64>> {
        let mut ids = Vec::new();
        let mut tx = self.pool.begin().await?;

        for item in items {
            let uuid = uuid::Uuid::new_v4().to_string();
            let now = chrono::Utc::now().timestamp();
            let created = item.created_at.unwrap_or(now);
            let sort_order = ids.len() as f64 + 1.0;

            let result = sqlx::query(
                r#"
                INSERT INTO tasks (uuid, title, status, priority, instance_id, creator_name, sort_order, created_at, updated_at)
                VALUES (?, ?, 'pending', 0, ?, 'migrated', ?, ?, ?)
                "#,
            )
            .bind(&uuid)
            .bind(&item.title)
            .bind(&item.instance_id)
            .bind(sort_order)
            .bind(created)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            ids.push(result.last_insert_rowid());
        }

        tx.commit().await?;
        info!("Migrated {} tasks from localStorage", ids.len());
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{MigrateTaskItem, Task, TaskListFilters, UpdateTaskRequest};
    use crate::repository::test_helpers;
    use chrono::Utc;

    fn make_task(title: &str, sort_order: f64) -> Task {
        let now = Utc::now().timestamp();
        Task {
            id: None,
            uuid: uuid::Uuid::new_v4().to_string(),
            title: title.to_string(),
            body: None,
            status: "pending".to_string(),
            priority: 0,
            instance_id: None,
            creator_id: None,
            creator_name: "test".to_string(),
            sort_order,
            created_at: now,
            updated_at: now,
            completed_at: None,
            is_deleted: false,
            sent_text: None,
            conversation_id: None,
        }
    }

    #[tokio::test]
    async fn create_and_get_task() {
        let repo = test_helpers::test_repository().await;
        let task = make_task("Fix the bug", 1.0);
        let id = repo.create_task(&task).await.unwrap();
        assert!(id > 0);

        let fetched = repo.get_task(id).await.unwrap().unwrap();
        assert_eq!(fetched.title, "Fix the bug");
        assert_eq!(fetched.status, "pending");
        assert_eq!(fetched.sort_order, 1.0);
    }

    #[tokio::test]
    async fn get_nonexistent_task() {
        let repo = test_helpers::test_repository().await;
        let result = repo.get_task(99999).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn update_task_title_and_status() {
        let repo = test_helpers::test_repository().await;
        let id = repo
            .create_task(&make_task("Old title", 1.0))
            .await
            .unwrap();

        repo.update_task(
            id,
            &UpdateTaskRequest {
                title: Some("New title".to_string()),
                status: Some("in_progress".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let fetched = repo.get_task(id).await.unwrap().unwrap();
        assert_eq!(fetched.title, "New title");
        assert_eq!(fetched.status, "in_progress");
    }

    #[tokio::test]
    async fn complete_task_sets_completed_at() {
        let repo = test_helpers::test_repository().await;
        let id = repo.create_task(&make_task("Task", 1.0)).await.unwrap();

        repo.update_task(
            id,
            &UpdateTaskRequest {
                status: Some("completed".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let fetched = repo.get_task(id).await.unwrap().unwrap();
        assert_eq!(fetched.status, "completed");
        assert!(fetched.completed_at.is_some());
    }

    #[tokio::test]
    async fn soft_delete_task() {
        let repo = test_helpers::test_repository().await;
        let id = repo
            .create_task(&make_task("To delete", 1.0))
            .await
            .unwrap();

        repo.delete_task(id).await.unwrap();

        // get_task filters is_deleted
        let result = repo.get_task(id).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn list_tasks_ordered_by_sort_order() {
        let repo = test_helpers::test_repository().await;
        repo.create_task(&make_task("Third", 3.0)).await.unwrap();
        repo.create_task(&make_task("First", 1.0)).await.unwrap();
        repo.create_task(&make_task("Second", 2.0)).await.unwrap();

        let filters = TaskListFilters {
            status: None,
            instance_id: None,
            tag: None,
            search: None,
            limit: None,
            offset: None,
        };
        let tasks = repo.list_tasks(&filters).await.unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].task.title, "First");
        assert_eq!(tasks[1].task.title, "Second");
        assert_eq!(tasks[2].task.title, "Third");
    }

    #[tokio::test]
    async fn list_tasks_filter_by_status() {
        let repo = test_helpers::test_repository().await;
        let id1 = repo.create_task(&make_task("Pending", 1.0)).await.unwrap();
        repo.create_task(&make_task("Also pending", 2.0))
            .await
            .unwrap();

        repo.update_task(
            id1,
            &UpdateTaskRequest {
                status: Some("completed".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let filters = TaskListFilters {
            status: Some("pending".to_string()),
            instance_id: None,
            tag: None,
            search: None,
            limit: None,
            offset: None,
        };
        let tasks = repo.list_tasks(&filters).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].task.title, "Also pending");
    }

    #[tokio::test]
    async fn task_tags() {
        let repo = test_helpers::test_repository().await;
        let id = repo
            .create_task(&make_task("Tagged task", 1.0))
            .await
            .unwrap();

        repo.add_task_tag(id, "bug").await.unwrap();
        repo.add_task_tag(id, "feature").await.unwrap();

        let filters = TaskListFilters {
            status: None,
            instance_id: None,
            tag: None,
            search: None,
            limit: None,
            offset: None,
        };
        let tasks = repo.list_tasks(&filters).await.unwrap();
        assert_eq!(tasks[0].tags.len(), 2);

        // Filter by tag
        let filters = TaskListFilters {
            status: None,
            instance_id: None,
            tag: Some("bug".to_string()),
            search: None,
            limit: None,
            offset: None,
        };
        let filtered = repo.list_tasks(&filters).await.unwrap();
        assert_eq!(filtered.len(), 1);
    }

    #[tokio::test]
    async fn remove_task_tag() {
        let repo = test_helpers::test_repository().await;
        let id = repo
            .create_task(&make_task("Tagged task", 1.0))
            .await
            .unwrap();

        // Add two tags
        repo.add_task_tag(id, "bug").await.unwrap();
        repo.add_task_tag(id, "feature").await.unwrap();

        // Find the tag_id for "bug"
        let filters = TaskListFilters {
            status: None,
            instance_id: None,
            tag: None,
            search: None,
            limit: None,
            offset: None,
        };
        let tasks = repo.list_tasks(&filters).await.unwrap();
        let bug_tag = tasks[0].tags.iter().find(|t| t.name == "bug").unwrap();
        let bug_tag_id = bug_tag.id;

        // Remove the "bug" tag
        repo.remove_task_tag(id, bug_tag_id).await.unwrap();

        // Verify only "feature" remains
        let tasks = repo.list_tasks(&filters).await.unwrap();
        assert_eq!(tasks[0].tags.len(), 1);
        assert_eq!(tasks[0].tags[0].name, "feature");
    }

    #[tokio::test]
    async fn remove_task_tag_nonexistent_is_noop() {
        let repo = test_helpers::test_repository().await;
        let id = repo.create_task(&make_task("No tags", 1.0)).await.unwrap();

        // Removing a non-existent tag should not error
        repo.remove_task_tag(id, 9999).await.unwrap();
    }

    #[tokio::test]
    async fn task_dispatch() {
        let repo = test_helpers::test_repository().await;
        let id = repo
            .create_task(&make_task("Dispatch me", 1.0))
            .await
            .unwrap();

        let dispatch = repo
            .create_task_dispatch(id, "inst-1", "fix this bug")
            .await
            .unwrap();
        assert_eq!(dispatch.task_id, id);
        assert_eq!(dispatch.instance_id, "inst-1");

        let dispatches = repo.get_dispatches_for_tasks(&[id]).await.unwrap();
        assert_eq!(dispatches.len(), 1);
        assert_eq!(dispatches[0].sent_text, "fix this bug");
    }

    #[tokio::test]
    async fn get_task_with_tags_includes_tags_and_dispatches() {
        let repo = test_helpers::test_repository().await;
        let id = repo
            .create_task(&make_task("Full task", 1.0))
            .await
            .unwrap();

        // Add tags
        repo.add_task_tag(id, "bug").await.unwrap();
        repo.add_task_tag(id, "urgent").await.unwrap();

        // Add a dispatch
        repo.create_task_dispatch(id, "inst-1", "fix it")
            .await
            .unwrap();

        // get_task_with_tags should return everything
        let twt = repo.get_task_with_tags(id).await.unwrap().unwrap();
        assert_eq!(twt.task.title, "Full task");
        assert_eq!(twt.tags.len(), 2);
        let tag_names: Vec<&str> = twt.tags.iter().map(|t| t.name.as_str()).collect();
        assert!(tag_names.contains(&"bug"));
        assert!(tag_names.contains(&"urgent"));
        assert_eq!(twt.dispatches.len(), 1);
        assert_eq!(twt.dispatches[0].sent_text, "fix it");
    }

    #[tokio::test]
    async fn get_task_with_tags_nonexistent_returns_none() {
        let repo = test_helpers::test_repository().await;
        let result = repo.get_task_with_tags(99999).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn get_task_with_tags_no_tags_no_dispatches() {
        let repo = test_helpers::test_repository().await;
        let id = repo
            .create_task(&make_task("Bare task", 1.0))
            .await
            .unwrap();

        let twt = repo.get_task_with_tags(id).await.unwrap().unwrap();
        assert_eq!(twt.task.title, "Bare task");
        assert!(twt.tags.is_empty());
        assert!(twt.dispatches.is_empty());
    }

    #[tokio::test]
    async fn get_next_sort_order() {
        let repo = test_helpers::test_repository().await;

        // No tasks → sort_order is 1.0
        let order = repo.get_next_sort_order(None).await.unwrap();
        assert_eq!(order, 1.0);

        repo.create_task(&make_task("A", 5.0)).await.unwrap();
        let order = repo.get_next_sort_order(None).await.unwrap();
        assert_eq!(order, 6.0);
    }

    #[tokio::test]
    async fn migrate_tasks() {
        let repo = test_helpers::test_repository().await;
        let items = vec![
            MigrateTaskItem {
                title: "Migrated 1".to_string(),
                instance_id: Some("inst-1".to_string()),
                created_at: Some(1000),
            },
            MigrateTaskItem {
                title: "Migrated 2".to_string(),
                instance_id: None,
                created_at: None,
            },
        ];
        let ids = repo.migrate_tasks(&items).await.unwrap();
        assert_eq!(ids.len(), 2);

        let task = repo.get_task(ids[0]).await.unwrap().unwrap();
        assert_eq!(task.title, "Migrated 1");
        assert_eq!(task.status, "pending");
        assert_eq!(task.creator_name, "migrated");
    }
}
