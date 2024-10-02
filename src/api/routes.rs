use actix_web::web;
use crate::api::handlers;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api")
            .service(web::resource("/register").route(web::post().to(handlers::register)))
            .service(web::resource("/login").route(web::post().to(handlers::login)))
            // Add more routes here
    );
}