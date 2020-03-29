#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate rocket;

extern crate clap;
extern crate image;
extern crate tera;

use std::collections::HashMap;
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
            ("image.html", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/image.html"))),
            ("index.html", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/index.html"))),
        ]).expect("couldn't add template to Tera");
        tera
    };
}

struct RootDir(PathBuf);

#[get("/_style.css")]
fn css() -> content::Css<&'static str> {
    content::Css(
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/style.css"))
    )
}

#[get("/_album.js")]
fn js() -> content::JavaScript<&'static str> {
    content::JavaScript(
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/album.js"))
    )
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
fn serve_page(path: PathBuf, rootdir: State<RootDir>) -> Option<content::Html<String>> {
    // Path can be:
    // "" (empty) for the root dir itself
    // "herp" for a subdirectory
    // "something.jpg" for an image page
    let root_path = rootdir.0.as_path();
    let full_path: PathBuf = root_path.join(&path);
    println!("{:?}", full_path);
    for stueck in path.iter() {
        println!("ohai: {:?}", stueck);
    }

    if full_path.is_dir() {
        let mut albums = HashMap::new();
        let mut images = Vec::new();

        for entry in std::fs::read_dir(full_path).ok()? {
            let entry = entry.ok()?;
            let entry_path_abs = entry.path();
            let entry_path_rel = path.join(entry.file_name());
            if entry_path_abs.is_dir() {
                if entry.file_name().to_string_lossy().starts_with(".") {
                    continue;
                }
                let album_imgs = std::fs::read_dir(&entry_path_abs).ok()?
                    .take(3)
                    .map(|x| x.expect("need dirEntries").path())
                    .filter(|p| p.is_file())
                    .map(|p| String::from(p.file_name().expect("can't stringify").to_string_lossy()))
                    .collect::<Vec<String>>();
                albums.insert(String::from(entry_path_rel.to_str()?), album_imgs);
            } else {
                images.push(String::from(entry.file_name().to_string_lossy()));
            }
        }

        let mut context = Context::new();
        context.insert("this_album", &path.to_string_lossy());
        context.insert("albums", &albums);
        context.insert("images", &images);
        Some(content::Html(
            TEMPLATES.render("index.html", &context)
                .expect("failed to render template")
        ))
    } else {
        let mut context = Context::new();
        context.insert("album", &path.parent().expect("fail dir").to_string_lossy());
        context.insert("image", &path.file_name().expect("fail name").to_string_lossy());
        Some(content::Html(
            TEMPLATES.render("image.html", &context)
                .expect("failed to render template")
        ))
    }

}

#[get("/")]
fn index(rootdir: State<RootDir>) -> Option<content::Html<String>> {
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
        .mount("/", routes![index, serve_page, serve_file, css, js])
        .launch();
}
