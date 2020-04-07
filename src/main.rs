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
use rocket::http::{ContentType, Status};
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
    ImageInMem(Vec<u8>),
}

impl ImageFromFileOrMem {
    fn from_path(path: PathBuf) -> Self {
        Self::ImageFile(path)
    }

    fn from_image(img: DynamicImage) -> Result<Self, image::ImageError> {
        let mut writer = std::io::BufWriter::new(Vec::new());
        img.write_to(&mut writer, ImageOutputFormat::Jpeg(90))?;
        Ok(Self::ImageInMem(writer.into_inner().expect("sad panda")))
    }
}

impl<'r> Responder<'r> for ImageFromFileOrMem {
    fn respond_to(self, req: &Request) -> response::Result<'r> {
        match self {
            // If it's a file, reuse NamedFile's Responder impl
            ImageFromFileOrMem::ImageFile(img_path) => {
                NamedFile::open(img_path).respond_to(req)
            }
            // If it's from Mem, serialize it as JPG into a Vec and serve that
            ImageFromFileOrMem::ImageInMem(img_data) => {
                response::Response::build()
                    .header(ContentType::JPEG)
                    .sized_body(std::io::Cursor::new(img_data))
                    .ok()
            }
        }
    }
}

#[derive(Debug)]
enum ImageServError {
    ImageError(image::ImageError),
    BadRequest(String),
    NotFound(String),
}

impl<'r> Responder<'r> for ImageServError {
    fn respond_to(self, req: &Request) -> response::Result<'r> {
        match self {
            ImageServError::NotFound(reason) => {
                response::status::NotFound(reason)
                    .respond_to(req)
            }
            ImageServError::BadRequest(reason) => {
                response::status::BadRequest(Some(reason))
                    .respond_to(req)
            }
            ImageServError::ImageError(err) => {
                response::status::Custom(
                    Status::InternalServerError,
                    format!("Image Processing Error: {:?}", err)
                ).respond_to(req)
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
fn serve_file(what: String, path: PathBuf, opts: State<Options>) -> Result<ImageFromFileOrMem, ImageServError> {
    let rootdir = &opts.root_dir;

    // What is either preview, thumb or img
    if what != "img" && what != "thumb" && what != "preview" {
        return Err(ImageServError::BadRequest("can only serve img, thumb or preview".to_owned()));
    }

    // Path is the path to the image relative to the root dir
    let img_path = rootdir.as_path().join(&path);

    if !img_path.exists() {
        return Err(ImageServError::NotFound("image does not exist".to_owned()));
    }

    if what == "img" {
        // Serve the image directly, without scaling
        return Ok(ImageFromFileOrMem::from_path(img_path));
    }

    // If path is a/b/c/d.jpg,         we'll place the thumbs/previews in
    //            a/b/c/.<what>/d.jpg
    // This is completely optional - if we can't do this for _any_ reason at all, we
    // just shrink the image in-memory and return the image data without saving it.
    // Thus the code is structured such that it tries to build the path we're going
    // to use, and along the way, makes sure that everything exists / is accessible.
    // If it hits any roadblocks, it just returns None. (Separate fn so ? does this.)
    fn get_scaled_img_path(rootdir: &PathBuf, path: &PathBuf, what: &String) -> Option<PathBuf> {
        // a/b/c/d.jpg -> a/b/c/.<what>
        let dir_path = rootdir.as_path()
            .join(path)
            .parent()?
            .join(".".to_owned() + what);
        // Make sure it exists - return None if we can't
        if !dir_path.exists() {
            std::fs::create_dir(&dir_path).ok()?;
        }
        // Make sure we're allowed to write to it
        if dir_path.metadata().ok()?.permissions().readonly() {
            return None;
        }
        // a/b/c/.<what> -> a/b/c/.<what>/d.jpg
        Some(dir_path.join(path.file_name()?))
    }

    let scaled_path = get_scaled_img_path(&rootdir, &path, &what);

    // Do we have that already as a file? If so, then return the file
    if scaled_path.is_some() && scaled_path.as_ref().unwrap().exists() {
        return Ok(ImageFromFileOrMem::from_path(scaled_path.unwrap()));
    }

    // We don't have a file, so we need to scale the source image down
    let img = image::open(&img_path)
        .map_err(ImageServError::ImageError)?;

    // Scale the image either to 1920x1080 for previews, or 350x250 for thumbnails.
    let (width, height) =
        if what == "thumb" {
            ( 350,  250)
        } else {
            (1920, 1080)
        };

    // If the original image is smaller than the "thumbnail" we're intending
    // to create, let's just be lazy and return the original
    if img.width() <= width && img.height() <= height {
        return Ok(ImageFromFileOrMem::from_path(img_path));
    }

    // Convert the image
    let thumbnail = img.thumbnail(width, height);

    // If we have a path, try to save the image. If that fails, no biggie
    if let Some(scaled_path) = scaled_path {
        let _ = thumbnail
            .save(&scaled_path);
    }

    ImageFromFileOrMem::from_image(thumbnail)
        .map_err(ImageServError::ImageError)
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
