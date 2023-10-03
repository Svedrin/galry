#![feature(proc_macro_hygiene, decl_macro, let_chains)]

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate rocket;

extern crate clap;
extern crate exif;
extern crate image;
extern crate tera;

use std::collections::HashMap;
use std::io;
use std::path::{Path,PathBuf};
use exif::Reader;
use rocket::State;
use rocket::request::Request;
use rocket::http::{ContentType, Status};
use rocket::response::{self,content,NamedFile,Responder};
use rand::seq::SliceRandom;
use structopt::StructOpt;
use tera::{Context, Tera};
use image::{GenericImageView, DynamicImage, ImageOutputFormat, ImageError};

lazy_static! {
    pub static ref TEMPLATES: Tera = {
        let mut tera = Tera::default();
        tera.add_raw_templates(vec![
            ("base.html",  include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/base.html"))),
            ("image.html", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/image.html"))),
            ("index.html", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/index.html"))),
        ]).expect("couldn't add template to Tera");
        tera.register_function("url_for", make_url_for);
        tera
    };
}

/// Teeny-tiny Image Gallery that fits into a single executable and does not require a database.
#[derive(StructOpt, Debug)]
struct Options {
    /// Directory with images to serve
    #[structopt(env="GALRY_ROOT_DIR", parse(from_os_str))]
    root_dir: PathBuf,

    /// Directory to store thumbnails in (defaults to root_dir)
    #[structopt(short, long, env="GALRY_THUMBS_DIR", parse(from_os_str))]
    thumbs_dir: Option<PathBuf>,

    /// Set this to have the zoom button in the Image view
    /// open the preview image rather than the original.
    #[structopt(short, long, env="GALRY_ZOOM_SHOWS_PREVIEW")]
    zoom_shows_preview: bool,

    /// Treat the file system as read-only and never write
    /// thumbnails or previews to disk.
    #[structopt(short, long, env="GALRY_READ_ONLY_FS")]
    read_only_fs: bool,
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
enum GalryError {
    ImageError(image::ImageError),
    IoError(io::Error),
    BadRequest(String),
    NotFound(String),
}

impl<'r> Responder<'r> for GalryError {
    fn respond_to(self, req: &Request) -> response::Result<'r> {
        match self {
            GalryError::NotFound(reason) => {
                response::status::NotFound(reason)
                    .respond_to(req)
            }
            GalryError::BadRequest(reason) => {
                response::status::BadRequest(Some(reason))
                    .respond_to(req)
            }
            GalryError::ImageError(err) => {
                response::status::Custom(
                    Status::InternalServerError,
                    format!("Image Processing Error: {:#?}", err)
                ).respond_to(req)
            }
            GalryError::IoError(err) => {
                response::status::Custom(
                    Status::InternalServerError,
                    format!("IO Error: {:#?}", err)
                ).respond_to(req)
            }
        }
    }
}

impl From<image::ImageError> for GalryError {
    fn from(err: ImageError) -> Self {
        Self::ImageError(err)
    }
}

impl From<io::Error> for GalryError {
    fn from(err: io::Error) -> Self {
        Self::IoError(err)
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

fn make_url_for(args: &HashMap<String, tera::Value>) -> tera::Result<tera::Value> {
    Ok(
        vec!["prefix", "album", "image"].into_iter()
            // for each of these ^, retrieve the value from `args`
            .map( |param| {
                args.get(param)
                    .and_then(|arg| tera::from_value::<String>(arg.to_owned()).ok())
                    .unwrap_or(String::from(""))
            })
            // reduce them by joining into a single PathBuf
            .fold(
                PathBuf::from("/"),
                |acc, cur| acc.join(cur)
            )
            // PathBuf -> String -> tera::Value
            .to_string_lossy()
            .into()
    )
}

#[get("/_/<what>/<path..>", rank=1)]
fn serve_file(what: String, path: PathBuf, opts: State<Options>) -> Result<ImageFromFileOrMem, GalryError> {
    let rootdir = &opts.root_dir;

    // What is either preview, thumb or img
    if what != "img" && what != "thumb" && what != "preview" {
        return Err(GalryError::BadRequest("can only serve img, thumb or preview".into()));
    }

    // Path is the path to the image relative to the root dir
    let img_path = rootdir.as_path().join(&path);

    if !img_path.exists() {
        return Err(GalryError::NotFound("image does not exist".into()));
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
    fn get_scaled_img_path(rootdir: &Path, path: &PathBuf, what: &str) -> Option<PathBuf> {
        // a/b/c/d.jpg -> a/b/c/.<what>
        let dir_path = rootdir
            .join(path)
            .parent()?
            .join(String::from(".") + what);
        // Make sure it exists - return None if we can't
        if !dir_path.exists() {
            std::fs::create_dir_all(&dir_path).ok()?;
        }
        // Make sure we're allowed to write to it
        if dir_path.metadata().ok()?.permissions().readonly() {
            return None;
        }
        // a/b/c/.<what> -> a/b/c/.<what>/d.jpg
        Some(dir_path.join(path.file_name()?))
    }

    let scaled_path =
        if opts.read_only_fs {
            None
        } else if let Some(thumbs_dir) = &opts.thumbs_dir {
            get_scaled_img_path(thumbs_dir, &path, &what)
        } else {
            get_scaled_img_path(rootdir, &path, &what)
        };

    // Do we have that already as a file? If so, then return the file
    if let Some(scaled_path) = &scaled_path && scaled_path.exists() {
        return Ok(ImageFromFileOrMem::from_path(scaled_path.clone()));
    }

    // We don't have a file, so we need to scale the source image down
    let img = image::open(&img_path)?;

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
            .save(scaled_path);
    }

    Ok(ImageFromFileOrMem::from_image(thumbnail)?)
}

fn get_album_images(album_dir: &Path) -> Vec<String> {
    std::fs::read_dir(album_dir)
        .map(|rd| {
            rd
                .filter_map(|entres| entres.ok())
                .filter(|ent| ent.path().is_file())
                .filter(|ent| ent.path().extension().is_some_and(
                    |ext| matches!(ext.to_ascii_lowercase().to_str(), Some("jpg")|Some("png"))
                ))
                .map(|ent| ent.file_name().to_string_lossy().into())
                .collect()
        })
        .unwrap_or_default()
}

#[get("/<path..>", rank=2)]
fn serve_page(path: PathBuf, opts: State<Options>) -> Result<content::Html<String>, GalryError> {
    let rootdir = &opts.root_dir;
    // Path can be:
    // "" (empty) for the root dir itself
    // "herp" for a subdirectory
    // "something.jpg" for an image page
    let root_path = rootdir.as_path();
    let full_path: PathBuf = root_path.join(&path);

    if !full_path.exists() {
        return Err(GalryError::NotFound(format!("path '{:#?}' does not exist", full_path)));
    }

    let breadcrumbs: Vec<(String, String)> = {
        let breadcrumbs_words = path.iter()
            .map(|p| p.to_string_lossy().into())
            .collect::<Vec<String>>();

        let mut path_so_far: PathBuf = "".into();
        let breadcrumbs_paths = path.iter()
            .map(|p| { path_so_far.push(p); path_so_far.to_string_lossy().into() })
            .collect::<Vec<String>>();

        breadcrumbs_words.into_iter()
            .zip(breadcrumbs_paths)
            .collect()
    };

    let mut context = Context::new();
    context.insert("crumbs", &breadcrumbs);
    context.insert(
        "rootdir",
        &root_path.file_name()
            .map(|rp| rp.to_string_lossy())
            .unwrap_or("/".into())
    );

    let mut rng = rand::thread_rng();

    if full_path.is_dir() {
        let mut albums = Vec::new();
        let mut images = Vec::new();

        let mut entries: Vec<_> = std::fs::read_dir(full_path)?
            .filter_map(|r| r.ok())
            .collect();
        entries.sort_by_key(|dir| dir.path());
        for entry in entries {
            if entry.file_name().to_string_lossy().starts_with('.') ||
                entry.file_name().to_string_lossy().eq_ignore_ascii_case("lost+found") {
                continue;
            }
            let entry_path_abs = entry.path();
            let entry_path_rel = path.join(entry.file_name());
            if entry_path_abs.is_dir() {
                let mut album_imgs = get_album_images(&entry_path_abs);
                album_imgs.shuffle(&mut rng);
                albums.push((
                    String::from(entry_path_rel.to_string_lossy()),
                    album_imgs.into_iter().take(3).collect::<Vec<_>>()
                ));
            } else if
                let Some(ext) = entry.path().extension() &&
                matches!(ext.to_ascii_lowercase().to_str(), Some("jpg")|Some("png"))
            {
                images.push(String::from(entry.file_name().to_string_lossy()));
            }
        }

        context.insert("this_album", &path.to_string_lossy());
        context.insert("albums", &albums);
        context.insert("images", &images);
        Ok(content::Html(
            TEMPLATES.render("index.html", &context)
                .expect("failed to render template")
        ))
    }
    else if full_path.is_file() {
        // Try to read EXIF data. This is optional, and if it fails for any reason, we just
        // serve the image without it.
        let exif = std::fs::File::open(&full_path).ok()
            .and_then(|file| {
                Reader::new()
                    .read_from_container(&mut std::io::BufReader::new(&file)).ok()
            })
            .map(|exif| {
                exif.fields()
                    .map(|field| (
                        field.tag.to_string(),
                        field.display_value().with_unit(&exif).to_string()
                    ))
                    .collect::<HashMap<String, String>>()
            });

        // Find previous and next image
        let mut album_imgs = get_album_images(full_path.parent().unwrap());
        album_imgs.sort();
        let this_img_pos = album_imgs.iter().position(
            |x| x == &full_path.file_name().unwrap().to_string_lossy()
        ).expect("img not in parent dir!?");
        let prev = this_img_pos.checked_sub(1).map(|i| &album_imgs[i]);
        let next = album_imgs.get(this_img_pos + 1);

        // "" if not path else (path + "/")
        let parent = path.parent()
            .and_then(|p| p.to_string_lossy().into())
            .unwrap_or("".into());
        context.insert("this_album", &parent);
        context.insert("image", &path.file_name().expect("fail name").to_string_lossy());
        context.insert("exif", &exif);
        context.insert("zoom_shows_preview", &opts.zoom_shows_preview);
        context.insert("prev_picture", &prev);
        context.insert("next_picture", &next);
        Ok(content::Html(
            TEMPLATES.render("image.html", &context)
                .expect("failed to render template")
        ))
    }
    else {
        Err(GalryError::NotFound(format!("path '{:#?}' is neither a file nor a directory", full_path)))
    }
}

#[get("/")]
fn index(opts: State<Options>) -> Result<content::Html<String>, GalryError> {
    serve_page(PathBuf::from(""), opts)
}

pub fn main() {
    rocket::ignite()
        .manage(Options::from_args())
        .mount("/", routes![index, serve_page, serve_file, css, js])
        .launch();
}
