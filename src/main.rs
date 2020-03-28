#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate rocket;

extern crate clap;
extern crate image;
extern crate tera;

use std::path::PathBuf;
use clap::{App, Arg};
use rocket::State;
use rocket::response::{content,NamedFile,Responder};
use tera::{Context, Tera};
use image::GenericImageView;

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

#[get("/_/<what>/<path..>", rank=1)]
fn serve_file(what: String, path: PathBuf, rootdir: State<RootDir>) -> Option<NamedFile> {
    // What is either preview, thumb or img
    if what != "img" && what != "thumb" && what != "preview" {
        return None;
    }

    // Path is the path to the image relative to the root dir
    let img_path = rootdir.0.as_path().join(&path);
    if what == "img" {
        // Serve the image directly, without scaling
        return NamedFile::open(img_path).ok();
    }

    // Scale the image either to 1920x1080 for previews, or 350x250 for thumbnails
    let dir_path = rootdir.0.as_path().join(&path)
        .parent().expect("file has no directory!?")
        .join(".".to_owned() + &what);

    let scaled_path = dir_path
        .join(path.file_name().expect("file without file name!?"));

    if !scaled_path.exists() {
        let img = image::open(&img_path).ok()?;
        let (width, height) =
            if what == "thumb" {
                ( 350,  250)
            } else {
                (1920, 1080)
            };
        if img.width() <= width && img.height() <= height {
            return NamedFile::open(img_path).ok();
        }

        // Make sure the output directory exists
        // TODO: if readonly, do not write it to a file but instead return it directly
        // This will require some refactoring so that this function can return
        // DynamicImage instances _as well as_ NamedFiles
        if !dir_path.exists() && !dir_path.parent()?.metadata().ok()?.permissions().readonly() {
            std::fs::create_dir(&dir_path).ok()?;
        }

        img.thumbnail(width, height)
            .save(&scaled_path)
            .ok()?;
    }

    NamedFile::open(scaled_path).ok()
}

#[get("/<path..>", rank=2)]
fn serve_page(path: PathBuf, rootdir: State<RootDir>) -> content::Html<String> {
    // Path can be:
    // "" (empty) for the root dir itself
    // "herp" for a subdirectory
    // "something.jpg" for an image page
    let full_path: PathBuf = rootdir.0.as_path().join(&path);
    println!("{:?}", full_path);
    for stueck in path.iter() {
        println!("ohai: {:?}", stueck);
    }

    let mut context = Context::new();
    context.insert("albums", "dunno");
    context.insert("imagejson", "'OMFG'");
    context.insert("images", "5");
    content::Html(
        TEMPLATES.render("base.html", &context)
            .expect("failed to render template")
    )
}

#[get("/")]
fn index(rootdir: State<RootDir>) -> content::Html<String> {
    serve_page(PathBuf::from(""), rootdir)
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
        .mount("/", routes![index, serve_page, serve_file, css])
        .launch();
}
