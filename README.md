# Meemuttaja

A small Rust + GPUI Kit desktop meme maker. Open an image or browse and search popular blank templates from Imgflip, click either caption field and type, adjust the text size, and export the result as a PNG. Captions are rendered in uppercase with a black outline; the exported image keeps the source dimensions.

## Run

Install a recent Rust toolchain and run:

```sh
cargo run --release
```

Supported source formats are PNG, JPEG, WebP, GIF (first frame), and BMP. The app looks for Impact or a bold system font on macOS, Linux, and Windows. Caption fields support normal text editing, including selection and paste.

The template browser requires an internet connection and loads its catalog and images from [Imgflip](https://imgflip.com/api).

## CI builds

GitHub Actions builds release binaries for Windows x64 and Apple Silicon macOS on pushes to `main`, pull requests, and manual runs. Download the binaries from the workflow run's artifacts. These are unsigned standalone executables, not installers or macOS app bundles.
