#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate rocket;

extern crate clap;
extern crate exif;
extern crate image;
extern crate tera;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use exif::Reader;
use rocket::State;
use rocket::request::Request;
use rocket::http::ContentType;
use rocket::response::{self,content,NamedFile,Responder};
use structopt::StructOpt;
use tera::{Context, Tera};
use image::{GenericImageView, DynamicImage, ImageOutputFormat};

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

/// Teeny-tiny Image Gallery that fits into a single executable and does not require a database.
#[derive(StructOpt, Debug)]
struct Options {
    /// Directory with images to serve
    #[structopt(env="GALRY_ROOT_DIR", parse(from_os_str))]
    root_dir: PathBuf,

    /// Set this to have the zoom button in the Image view
    /// open the preview image rather than the original.
    #[structopt(short, long, env="GALRY_ZOOM_SHOWS_PREVIEW")]
    zoom_shows_preview: bool,
}

/// Allow the server to return an Image either from a file or from memory
enum ImageFromFileOrMem {
    ImageFile(PathBuf),
    ImageInMem(DynamicImage),
}

impl<'r> Responder<'r> for ImageFromFileOrMem {
    fn respond_to(self, req: &Request) -> response::Result<'r> {
        match self {
            // If it's a file, reuse NamedFile's Responder impl
            ImageFromFileOrMem::ImageFile(img_path) => {
                NamedFile::open(img_path).respond_to(req)
            }
            // If it's from Mem, serialize it as JPG into a Vec and serve that
            ImageFromFileOrMem::ImageInMem(dyn_image) => {
                let img_data = {
                    let mut writer = std::io::BufWriter::new(Vec::new());
                    dyn_image.write_to(&mut writer, ImageOutputFormat::Jpeg(90))
                        .expect("cannot convert to jpg");
                    writer.into_inner()
                        .expect("sad panda")
                };
                response::Response::build()
                    .header(ContentType::JPEG)
                    .sized_body(std::io::Cursor::new(img_data))
                    .ok()
            }
        }
    }
}

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
fn serve_file(what: String, path: PathBuf, opts: State<Options>) -> Option<ImageFromFileOrMem> {
    let rootdir = &opts.root_dir;

    // What is either preview, thumb or img
    if what != "img" && what != "thumb" && what != "preview" {
        return None;
    }

    // Path is the path to the image relative to the root dir
    let img_path = rootdir.as_path().join(&path);
    if what == "img" {
        // Serve the image directly, without scaling
        return Some(ImageFromFileOrMem::ImageFile(img_path));
    }

    // Scale the image either to 1920x1080 for previews, or 350x250 for thumbnails
    let dir_path = rootdir.as_path().join(&path)
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
            return Some(ImageFromFileOrMem::ImageFile(img_path));
        }

        let thumbnail = img.thumbnail(width, height);

        // Make sure the .preview or .thumb directory exists.
        // If we cannot create it, return the image from memory.
        if !dir_path.exists() {
            if dir_path.parent()?.metadata().ok()?.permissions().readonly() {
                return Some(ImageFromFileOrMem::ImageInMem(thumbnail));
            }
            std::fs::create_dir(&dir_path).ok()?;
        }

        // Make sure we have write permission on the .preview and .thumb dirs.
        // If we do not, return the image from memory.
        if dir_path.metadata().ok()?.permissions().readonly() {
            return Some(ImageFromFileOrMem::ImageInMem(thumbnail));
        }

        // Save the image to disk and return it from file.
        thumbnail
            .save(&scaled_path)
            .ok()?;
    }

    Some(ImageFromFileOrMem::ImageFile(scaled_path))
}

#[get("/<path..>", rank=2)]
fn serve_page(path: PathBuf, opts: State<Options>) -> Option<content::Html<String>> {
    let rootdir = &opts.root_dir;
    // Path can be:
    // "" (empty) for the root dir itself
    // "herp" for a subdirectory
    // "something.jpg" for an image page
    let root_path = rootdir.as_path();
    let full_path: PathBuf = root_path.join(&path);

    let breadcrumbs: Vec<(String, String)> = {
        let breadcrumbs_words = path.iter()
            .map(|p| p.to_string_lossy().into())
            .collect::<Vec<String>>();

        let mut path_so_far: PathBuf = "".into();
        let breadcrumbs_paths = path.iter()
            .map(|p| { path_so_far.push(p); path_so_far.to_string_lossy().into() })
            .collect::<Vec<String>>();

        breadcrumbs_words.into_iter()
            .zip(breadcrumbs_paths.into_iter())
            .collect()
    };

    let mut context = Context::new();
    context.insert("crumbs", &breadcrumbs);
    context.insert("rootdir", &root_path.file_name()?.to_string_lossy());

    if full_path.is_dir() {
        let mut albums = Vec::new();
        let mut images = Vec::new();

        let mut entries: Vec<_> = std::fs::read_dir(full_path)
            .ok()?
            .map(|r| r.expect("need dirEntries"))
            .collect();
        entries.sort_by_key(|dir| dir.path());
        for entry in entries {
            let entry_path_abs = entry.path();
            let entry_path_rel = path.join(entry.file_name());
            if entry_path_abs.is_dir() {
                if entry.file_name().to_string_lossy().starts_with(".") ||
                   entry.file_name().to_string_lossy().eq_ignore_ascii_case("lost+found") {
                    continue;
                }
                let album_imgs = std::fs::read_dir(&entry_path_abs).ok()?
                    .take(3)
                    .map(|x| x.expect("need dirEntries").path())
                    .filter(|p| p.is_file())
                    .map(|p| String::from(p.file_name().expect("can't stringify").to_string_lossy()))
                    .collect::<Vec<String>>();
                albums.push((String::from(entry_path_rel.to_str()?), album_imgs));
            } else {
                images.push(String::from(entry.file_name().to_string_lossy()));
            }
        }

        // "" if not path else (path + "/")
        context.insert("this_album", & match path.to_string_lossy() {
            Cow::Borrowed("") => "".into(),
            path_str @ _ => format!("{}/", path_str)
        });
        context.insert("albums", &albums);
        context.insert("images", &images);
        Some(content::Html(
            TEMPLATES.render("index.html", &context)
                .expect("failed to render template")
        ))
    } else {
        let file = std::fs::File::open(&full_path).ok()?;
        let exif = Reader::new()
            .read_from_container(&mut std::io::BufReader::new(&file)).ok()?;
        let mut strexif = HashMap::new();
        for f in exif.fields() {
            strexif.insert(f.tag.to_string(), f.display_value().with_unit(&exif).to_string());
        }

        // "" if not path else (path + "/")
        context.insert("this_album", & match path.parent()?.to_string_lossy() {
            Cow::Borrowed("") => "".into(),
            path_str @ _ => format!("{}/", path_str)
        });
        context.insert("image", &path.file_name().expect("fail name").to_string_lossy());
        context.insert("exif", &strexif);
        context.insert("zoom_shows_preview", &opts.zoom_shows_preview);
        Some(content::Html(
            TEMPLATES.render("image.html", &context)
                .expect("failed to render template")
        ))
    }

}

#[get("/")]
fn index(opts: State<Options>) -> Option<content::Html<String>> {
    serve_page(PathBuf::from(""), opts)
}

pub fn main() {
    rocket::ignite()
        .manage(Options::from_args())
        .mount("/", routes![index, serve_page, serve_file, css, js])
        .launch();
}
