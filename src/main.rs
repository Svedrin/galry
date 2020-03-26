//! An introduction to fundamental `Router` and `Router Builder` concepts to create a routing tree.
#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate tera;

use gotham::router::builder::*;
use gotham::router::Router;
use gotham::state::State;
use tera::{Context, Tera};

lazy_static! {
    pub static ref TERA: Tera =
        compile_templates!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/**/*"));
}


pub fn css(state: State) -> (State, &'static str) {
    (state, include_str!("../templates/style.css"))
}

/// Create a `Handler` that is invoked for requests to the path "/"
pub fn say_hello(state: State) -> (State, (mime::Mime, String)) {
    let mut context = Context::new();
    context.insert("albums", "dunno");
    context.insert("imagejson", "'OMFG'");
    context.insert("images", "5");
    context.insert("size", "3");
    context.insert("count", "10");
    let rendered = TERA.render("base.html", &context).unwrap();
    (state, (mime::TEXT_HTML, rendered))
}

fn router() -> Router {
    build_simple_router(|route| {
        // For the path "/" invoke the handler "say_hello"
        route.get("/").to(say_hello);
        route.get("/style.css").to(css);
    })
}

pub fn main() {
    let addr = "127.0.0.1:7878";
    println!("Listening for requests at http://{}", addr);

    // All incoming requests are delegated to the router for further analysis and dispatch
    gotham::start(addr, router())
}
