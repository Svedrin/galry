#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate rocket;

extern crate clap;
extern crate tera;

use std::path::{Path,PathBuf};
use clap::{App, Arg};
use rocket::response::content;
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

struct RootDir(PathBuf);

#[get("/_style.css")]
fn css() -> &'static str {
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/style.css"))
}

#[get("/<path..>")]
pub fn serve_path(path: PathBuf) -> content::Html<String> {
    println!("ohai: {:?}", path);
    let mut context = Context::new();
    context.insert("albums", "dunno");
    context.insert("imagejson", "'OMFG'");
    context.insert("images", "5");
    context.insert("size", "3");
    context.insert("count", "10");
    content::Html(
        TEMPLATES.render("base.html", &context)
            .expect("failed to render template")
    )
}

#[get("/")]
pub fn index() -> content::Html<String> {
    serve_path(PathBuf::from(""))
}

pub fn main() {
    let matches = App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about("single-binary image gallery")
        .arg(Arg::with_name("directory")
            .help("Directory with images to serve")
            .required(true)
            .index(1))
        .get_matches();

    rocket::ignite()
        .manage(RootDir(PathBuf::from(
            matches
                .value_of("directory")
                .expect("couldn't get directory arg")
        )))
        .mount("/", routes![index, serve_path, css])
        .launch();
}
