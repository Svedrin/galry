# Galry

Teeny-tiny Image Gallery that fits into a single executable and does not require a database.

Have a directory full of images and want it served as a useable website without any hassle at all? Then Galry is for you!

Docker: [svedrin/galry](https://hub.docker.com/r/svedrin/galry)

# Configuration

Galry does not require much configuration. If you're using the Docker container, just mount your image folder to `/pictures` and you're good to go.

Galry supports a few command line options, each of which can also be configured through an env var:

* `-r`, `--read-only-fs` (env: `GALRY_READ_ONLY_FS=true`): Treat the file system as read-only and never write thumbnails or previews to disk.

* `-t`, `--thumbs-dir` (env: `GALRY_THUMBS_DIR=/somedir`): Directory to store thumbnails in (defaults to `root_dir`).

* `-z`, `--zoom-shows-preview` (env: `GALRY_ZOOM_SHOWS_PREVIEW=true`): Set this to have the zoom button in the Image view open the preview image rather than the original.

The latter two options can be useful when serving images from slow media (such as an NFS share or HDDs that spin down). By saving the Thumbnails on an SSD and enabling `-z`, Galry will load the image only once from the slow disks and then grab them from the cache on subsequent requests.  

# Building galry on local host

You need to have the nightly toolchain installed to build galry:

```
rustup install nightly
cargo +nightly build --release
```

# Building galry in Docker

To create a galry docker image without needing to have rust installed locally, the ```Dockerfile.build``` will use a mulistage build to build galry in an alpine/rust container and put the result in a plain alpine container ready to run.

```
docker build -t galry -f Dockerfile.build .
```

# Running Docker image

```
docker run --rm -p 8000:8000 -v <path_to_images>:/pictures:ro svedrin/galry
```

Replace ```svedrin/galry``` with just ```galry``` if you used the ```Dockerfile.build``` to build your own docker image.
