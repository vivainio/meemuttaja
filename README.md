# Meemuttaja

A Rust + GPUI Kit desktop meme maker. Open an image or browse blank templates from Imgflip, add and drag text blocks, and import images or search Wikimedia Commons for stickers. Select a block or sticker to resize or delete it, then export the result as a PNG or copy it to the clipboard. Captions are uppercase with a black outline; exports keep the source dimensions.

## Run

Install a recent Rust toolchain and run:

```sh
cargo run --release
```

Supported source formats are PNG, JPEG, WebP, GIF (first frame), and BMP. The app looks for Impact or a bold system font on macOS, Linux, and Windows. Caption fields support normal text editing, including selection and paste.

The template browser requires an internet connection and loads its catalog and images from [Imgflip](https://imgflip.com/api).

## Releases

Publishing a GitHub release triggers builds for Windows x64 and Apple Silicon macOS. The binaries are attached to that release. The macOS executable is distributed in a tarball to preserve its executable permission. They are unsigned standalone executables, not installers or macOS app bundles.

Online sticker search uses Wikimedia Commons. Images are added at thumbnail resolution and may have opaque backgrounds. Use each result’s source link to check its license and attribution requirements.
