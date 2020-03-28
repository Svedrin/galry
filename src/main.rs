#[macro_use] extern crate lazy_static;
extern crate clap;
extern crate tera;

use clap::{App, Arg};
use gotham::router::builder::*;
use gotham::router::Router;
use gotham::state::State;
use tera::{Context, Tera};

lazy_static! {
    pub static ref TEMPLATES: Tera = {
        let mut tera = Tera::default();
        tera.add_raw_templates(vec![
            ("base.html",  include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/base.html"))),
            ("index.html", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/index.html"))),
        ]).expect("couldn't add template to Tera");
        tera
    };
}

pub fn css(state: State) -> (State, &'static str) {
    (state, include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/style.css")))
}

/// Create a `Handler` that is invoked for requests to the path "/"
pub fn say_hello(state: State) -> (State, (mime::Mime, String)) {
    let mut context = Context::new();
    context.insert("albums", "dunno");
    context.insert("imagejson", "'OMFG'");
    context.insert("images", "5");
    context.insert("size", "3");
    context.insert("count", "10");
    let rendered = TEMPLATES.render("base.html", &context).unwrap();
    (state, (mime::TEXT_HTML, rendered))
}

fn router() -> Router {
    build_simple_router(|route| {
        // For the path "/" invoke the handler "say_hello"
        route.get("/").to(say_hello);
        route.get("/_style.css").to(css);
    })
}

pub fn main() {
    let matches = App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about("single-binary image gallery")
        .arg(Arg::with_name("port")
            .short("p")
            .long("port")
            .takes_value(true)
            .help("Port number to use [8080]"))
        .arg(Arg::with_name("directory")
            .help("Directory with images to serve")
            .required(true)
            .index(1))
        .get_matches();

    let addr = format!(
        ":::{}",
        matches.value_of("port").unwrap_or("8080").parse::<u16>()
            .expect("Port argument must be a number between 1 and 65535")
    );
    println!("Listening for requests at http://{}", addr);
    gotham::start(addr, router())
}
