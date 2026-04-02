pub mod browser;
pub mod reader;
pub mod search;
pub mod types;

// Re-export handlers for route registration
pub use browser::list_instance_files;
pub use reader::get_instance_file_content;
pub use search::search_instance_files;
