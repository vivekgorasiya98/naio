//! `neko ahiru` subcommands — project wizard, serve, migrate, routes.

mod add;
mod bench;
mod console;
mod create;
mod db;
mod doctor;
mod generate;
mod openapi;
mod serve;
mod test_cmd;
mod worker;

pub use bench::run_bench;
pub use create::run_create;
pub use serve::{run_migrate, run_routes, run_serve, ServeFlags};

pub use add::run_add;
pub use console::run_console;
pub use db::{run_db_migrate, run_db_reset, run_db_rollback, run_db_seed, run_db_status};
pub use doctor::run_doctor;
pub use generate::run_generate_resource;
pub use openapi::run_openapi;
pub use test_cmd::run_ahiru_test;
pub use worker::run_worker;
