pub mod api;
pub mod models;
pub mod services;
pub mod utils;

use std::error::Error;

pub async fn run() -> Result<(), Box<dyn Error>> {
    // Initialize services, start the web server, etc.
    Ok(())
}