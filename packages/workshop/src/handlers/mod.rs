pub mod admin;
pub mod browse;
pub mod bug_report;
pub mod conversations;
pub mod health;
pub mod inbox;
pub mod instances;
pub mod notes;
pub mod settings;
pub mod tasks;
pub mod websocket;

// Re-export all handlers for easy route registration
pub use admin::{
    create_server_invite_handler, create_user_handler, delete_user_handler, get_config_handler,
    get_database_stats, list_server_invites_handler, list_users_handler, patch_config_handler,
    restart_handler, revoke_server_invite_handler, trigger_import, update_user_handler,
};
pub use browse::{browse_directory, create_directory, create_worktree, git_detailed_info};
pub use bug_report::create_bug_report;
pub use conversations::{
    add_comment, create_share, extract_title_from_turn, format_progress_event,
    format_turn_with_attribution, get_comments, get_conversation, get_conversation_by_id,
    get_shared_conversation, list_conversations, poll_conversation, search_conversations_handler,
};
pub use health::{health_handler, health_live_handler, health_ready_handler, metrics_handler};
pub use inbox::{dismiss_inbox_handler, list_inbox_handler};
pub use instances::{
    accept_invitation, create_instance, create_invitation, delete_instance, get_instance,
    get_instance_output, list_instances, remove_collaborator, set_custom_name,
};
pub use notes::{create_note, delete_note, get_notes, update_note};
pub use settings::{get_user_settings_handler, update_user_settings_handler};
pub use tasks::{
    add_task_tag_handler, create_dispatch_handler, create_task_handler, delete_task_handler,
    get_task_handler, list_tasks_handler, migrate_tasks_handler, remove_task_tag_handler,
    send_task_handler, update_task_handler,
};
pub use websocket::multiplexed_websocket_handler;
