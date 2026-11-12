# Meemuttaja

A small Rust + GPUI Kit desktop meme maker. Open an image or browse and search popular blank templates from Imgflip, click either caption field and type, adjust the text size, and export the result as a PNG. Captions are rendered in uppercase with a black outline; the exported image keeps the source dimensions.

## Run

Install a recent Rust toolchain and run:

```sh
cargo run --release
```

Supported source formats are PNG, JPEG, WebP, GIF (first frame), and BMP. The app looks for Impact or a bold system font on macOS, Linux, and Windows. Caption fields support normal text editing, including selection and paste.

The template browser requires an internet connection and loads its catalog and images from [Imgflip](https://imgflip.com/api).

## Releases

Publishing a GitHub release triggers builds for Windows x64 and Apple Silicon macOS. The binaries are attached to that release. The macOS executable is distributed in a tarball to preserve its executable permission. They are unsigned standalone executables, not installers or macOS app bundles.
