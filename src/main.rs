use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{prelude::*, *};
use image::{DynamicImage, Rgba};
use imageproc::drawing::draw_text_mut;
use serde::Deserialize;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Deserialize)]
struct Template {
    name: String,
    url: String,
}
#[derive(Deserialize)]
struct CatalogResponse {
    success: bool,
    data: CatalogData,
}
#[derive(Deserialize)]
struct CatalogData {
    memes: Vec<Template>,
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("Meemuttaja/0.1 (https://github.com/vivainio/meemuttaja)")
        .build()
        .map_err(|e| e.to_string())
}

fn fetch_catalog() -> Result<Vec<Template>, String> {
    let response: CatalogResponse = client()?
        .get("https://api.imgflip.com/get_memes")
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json())
        .map_err(|e| e.to_string())?;
    if response.success {
        Ok(response.data.memes)
    } else {
        Err("Imgflip could not provide templates".into())
    }
}

fn fetch_template(url: &str) -> Result<DynamicImage, String> {
    let response = client()?
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    if response.content_length().is_some_and(|n| n > 20_000_000) {
        return Err("Template image is too large".into());
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    response
        .take(20_000_001)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 20_000_000 {
        return Err("Template image is too large".into());
    }
    image::load_from_memory(&bytes).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
struct CommonsResponse {
    query: Option<CommonsQuery>,
}
#[derive(Deserialize)]
struct CommonsQuery {
    pages: Vec<CommonsPage>,
}
#[derive(Deserialize)]
struct CommonsPage {
    title: String,
    index: Option<usize>,
    imageinfo: Option<Vec<CommonsImageInfo>>,
}
#[derive(Deserialize)]
struct CommonsImageInfo {
    thumburl: Option<String>,
    descriptionurl: String,
}
#[derive(Clone)]
struct StickerResult {
    title: String,
    thumbnail_url: String,
    source_url: String,
    thumbnail: Option<Arc<RenderImage>>,
}

fn search_commons(query: &str) -> Result<Vec<StickerResult>, String> {
    let terms = format!("{} filetype:bitmap", query.trim());
    let response: CommonsResponse = client()?
        .get("https://commons.wikimedia.org/w/api.php")
        .query(&[
            ("action", "query"),
            ("generator", "search"),
            ("gsrsearch", terms.as_str()),
            ("gsrnamespace", "6"),
            ("gsrlimit", "20"),
            ("prop", "imageinfo"),
            ("iiprop", "url"),
            ("iiurlwidth", "512"),
            ("format", "json"),
            ("formatversion", "2"),
        ])
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json())
        .map_err(|e| e.to_string())?;
    let mut pages = response.query.map(|q| q.pages).unwrap_or_default();
    pages.sort_by_key(|p| p.index.unwrap_or(usize::MAX));
    Ok(pages
        .into_iter()
        .filter_map(|page| {
            let info = page.imageinfo?.into_iter().next()?;
            Some(StickerResult {
                title: page.title.trim_start_matches("File:").to_string(),
                thumbnail_url: info.thumburl?,
                source_url: info.descriptionurl,
                thumbnail: None,
            })
        })
        .collect())
}

#[derive(Clone, Copy)]
enum Anchor {
    Top,
    Bottom,
    Center,
}

#[derive(Clone)]
struct CaptionData {
    text: String,
    size: f32,
    anchor: Anchor,
    offset: (f32, f32),
}
struct Caption {
    id: usize,
    data: CaptionData,
    input: Entity<InputState>,
    _subscription: Subscription,
}

#[derive(Clone)]
struct Sticker {
    image: Arc<DynamicImage>,
    position: (f32, f32),
    size: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Selection {
    Caption(usize),
    Sticker(usize),
}

struct MemeApp {
    image: Option<DynamicImage>,
    templates: Vec<Template>,
    browsing: bool,
    browsing_stickers: bool,
    sticker_query: String,
    sticker_input: Entity<InputState>,
    sticker_results: Vec<StickerResult>,
    sticker_search_id: u64,
    sticker_loading: bool,
    loading: bool,
    request_id: u64,
    search_input: Entity<InputState>,
    search: String,
    preview: Option<Arc<RenderImage>>,
    preview_base: Option<Arc<DynamicImage>>,
    preview_generation: u64,
    preview_busy: bool,
    captions: Vec<Caption>,
    next_caption_id: usize,
    selected: Option<Selection>,
    stickers: Vec<Sticker>,
    _subscriptions: Vec<Subscription>,
    status: String,
    drag: Option<(Selection, Point<Pixels>, (f32, f32), f32, f32)>,
    preview_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

fn preview_pixels(mut image: image::RgbaImage) -> image::RgbaImage {
    // GPUI RenderImage expects BGRA bytes; image processing and export use RGBA.
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    image
}

fn font() -> Option<FontArc> {
    [
        "/System/Library/Fonts/Supplemental/Impact.ttf",
        "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "C:\\Windows\\Fonts\\impact.ttf",
    ]
    .iter()
    .find_map(|p| {
        std::fs::read(p)
            .ok()
            .and_then(|bytes| FontArc::try_from_vec(bytes).ok())
    })
}

fn wrap_caption(text: &str, font: &FontArc, scale: PxScale, w: u32, margin: i32) -> Vec<String> {
    let text = text.to_uppercase();
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if imageproc::drawing::text_size(scale, &font, &candidate).0
            > w.saturating_sub((margin * 2) as u32)
            && !line.is_empty()
        {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn make_meme(
    source: &DynamicImage,
    captions: &[CaptionData],
    stickers: &[Sticker],
) -> Result<DynamicImage, String> {
    let font = font().ok_or("Could not find a bold system font")?;
    let mut out = source.to_rgba8();
    let (w, h) = out.dimensions();
    for sticker in stickers {
        let target_width = (w as f32 * sticker.size).round().max(1.) as u32;
        let target_height = ((target_width as f32 * sticker.image.height() as f32
            / sticker.image.width() as f32)
            .round() as u32)
            .max(1);
        let resized = sticker
            .image
            .resize_exact(
                target_width,
                target_height,
                image::imageops::FilterType::Triangle,
            )
            .to_rgba8();
        let x = (sticker.position.0 * w as f32 - target_width as f32 / 2.).round() as i64;
        let y = (sticker.position.1 * h as f32 - target_height as f32 / 2.).round() as i64;
        image::imageops::overlay(&mut out, &resized, x, y);
    }
    for caption in captions {
        let scale = PxScale::from((w as f32 * caption.size / 1000.0).max(8.0));
        let line_height = (font.as_scaled(scale).height() * 1.1).ceil() as i32;
        let margin = (w as f32 * 0.025) as i32;
        let lines = wrap_caption(&caption.text, &font, scale, w, margin);
        let block_height = line_height * lines.len() as i32;
        let start_y = match caption.anchor {
            Anchor::Top => margin,
            Anchor::Bottom => h as i32 - ((h as f32 * 0.08) as i32).max(margin) - block_height,
            Anchor::Center => (h as i32 - block_height) / 2,
        } + (caption.offset.1 * h as f32).round() as i32;
        for (i, line) in lines.iter().enumerate() {
            let tw = imageproc::drawing::text_size(scale, &font, line).0 as i32;
            let x = (w as i32 - tw) / 2 + (caption.offset.0 * w as f32).round() as i32;
            let y = start_y + i as i32 * line_height;
            let stroke = (scale.y / 18.0).round().max(1.0) as i32;
            for dy in -stroke..=stroke {
                for dx in -stroke..=stroke {
                    if dx * dx + dy * dy <= stroke * stroke {
                        draw_text_mut(
                            &mut out,
                            Rgba([0, 0, 0, 255]),
                            x + dx,
                            y + dy,
                            scale,
                            &font,
                            line,
                        );
                    }
                }
            }
            draw_text_mut(
                &mut out,
                Rgba([255, 255, 255, 255]),
                x,
                y,
                scale,
                &font,
                line,
            );
        }
    }
    Ok(DynamicImage::ImageRgba8(out))
}

impl MemeApp {
    fn image_rect(&self) -> Option<(f32, f32, f32, f32)> {
        let bounds = self.preview_bounds.get()?;
        let image = self.preview_base.as_ref()?;
        let width: f32 = bounds.size.width.into();
        let height: f32 = bounds.size.height.into();
        let scale = (width / image.width() as f32).min(height / image.height() as f32);
        let w = image.width() as f32 * scale;
        let h = image.height() as f32 * scale;
        Some((
            f32::from(bounds.origin.x) + (width - w) / 2.,
            f32::from(bounds.origin.y) + (height - h) / 2.,
            w,
            h,
        ))
    }
    fn begin_drag(&mut self, event: &MouseDownEvent) {
        let (x, y, w, h) = match self.image_rect() {
            Some(rect) => rect,
            None => return,
        };
        let px = f32::from(event.position.x) - x;
        let py = f32::from(event.position.y) - y;
        if px < 0. || px > w || py < 0. || py > h {
            return;
        }
        let Some(image) = &self.preview_base else {
            return;
        };
        let Some(font) = font() else { return };
        let iw = image.width() as f32;
        let ih = image.height() as f32;
        let margin = iw * 0.025;
        for caption in self.captions.iter().rev() {
            if caption.data.text.trim().is_empty() {
                continue;
            }
            let scale = PxScale::from((iw * caption.data.size / 1000.).max(8.));
            let line_height = (font.as_scaled(scale).height() * 1.1).ceil();
            let lines = wrap_caption(
                &caption.data.text,
                &font,
                scale,
                image.width(),
                margin as i32,
            );
            let block_height = line_height * lines.len() as f32;
            let base_y = match caption.data.anchor {
                Anchor::Top => margin,
                Anchor::Bottom => ih - (ih * 0.08).max(margin) - block_height,
                Anchor::Center => (ih - block_height) / 2.,
            };
            let offset = caption.data.offset;
            let text_width = lines
                .iter()
                .map(|line| imageproc::drawing::text_size(scale, &font, line).0 as f32)
                .fold(0., f32::max);
            let image_x = px / w * iw;
            let image_y = py / h * ih;
            if image_x >= (iw - text_width) / 2. + offset.0 * iw - margin
                && image_x <= (iw + text_width) / 2. + offset.0 * iw + margin
                && image_y >= base_y + offset.1 * ih - margin
                && image_y <= base_y + offset.1 * ih + block_height + margin
            {
                let selection = Selection::Caption(caption.id);
                self.selected = Some(selection);
                self.drag = Some((selection, event.position, offset, w, h));
                return;
            }
        }
        for (index, sticker) in self.stickers.iter().enumerate().rev() {
            let sw = sticker.size;
            let sh = sw * iw / ih * sticker.image.height() as f32 / sticker.image.width() as f32;
            let (sx, sy) = sticker.position;
            if (px / w - sx).abs() <= sw / 2. && (py / h - sy).abs() <= sh / 2. {
                let selection = Selection::Sticker(index);
                self.selected = Some(selection);
                self.drag = Some((selection, event.position, sticker.position, w, h));
                return;
            }
        }
    }
    fn add_caption(&mut self, anchor: Anchor, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.next_caption_id;
        self.next_caption_id += 1;
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Enter text"));
        let observed = input.clone();
        let subscription =
            cx.subscribe_in(&input, window, move |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Some(caption) = this.captions.iter_mut().find(|c| c.id == id) {
                        caption.data.text = observed.read(cx).value().to_string();
                        this.selected = Some(Selection::Caption(id));
                        this.refresh(cx);
                    }
                }
            });
        self.captions.push(Caption {
            id,
            data: CaptionData {
                text: String::new(),
                size: 90.,
                anchor,
                offset: (0., 0.),
            },
            input,
            _subscription: subscription,
        });
        self.selected = Some(Selection::Caption(id));
        cx.notify();
    }
    fn search_stickers(&mut self, cx: &mut Context<Self>) {
        let query = self.sticker_query.trim().to_string();
        if query.is_empty() {
            self.status = "Enter a search term.".into();
            cx.notify();
            return;
        }
        self.sticker_search_id += 1;
        let search_id = self.sticker_search_id;
        self.sticker_loading = true;
        self.sticker_results.clear();
        self.status = "Searching Wikimedia Commons…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { search_commons(&query) })
                .await;
            let urls = this
                .update(cx, |this, cx| {
                    if this.sticker_search_id != search_id {
                        return Vec::new();
                    }
                    this.sticker_loading = false;
                    match result {
                        Ok(results) => {
                            this.status =
                                format!("{} results from Wikimedia Commons", results.len());
                            this.sticker_results = results;
                        }
                        Err(error) => {
                            this.status = format!("Search failed: {error}");
                            cx.notify();
                            return Vec::new();
                        }
                    }
                    cx.notify();
                    this.sticker_results
                        .iter()
                        .map(|r| r.thumbnail_url.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for (index, url) in urls.into_iter().enumerate() {
                let this = this.clone();
                cx.spawn(async move |cx| {
                    let result = cx
                        .background_spawn(async move {
                            fetch_template(&url).map(|image| preview_pixels(image.to_rgba8()))
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if this.sticker_search_id == search_id {
                            if let (Some(item), Ok(image)) =
                                (this.sticker_results.get_mut(index), result)
                            {
                                item.thumbnail =
                                    Some(Arc::new(RenderImage::new(vec![image::Frame::new(
                                        image,
                                    )])));
                                cx.notify();
                            }
                        }
                    });
                })
                .detach();
            }
        })
        .detach();
    }
    fn use_online_sticker(&mut self, item: StickerResult, cx: &mut Context<Self>) {
        self.status = format!("Downloading {}…", item.title);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    fetch_template(&item.thumbnail_url).map(|image| (image, item.title))
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok((image, title)) => {
                    this.stickers.push(Sticker {
                        image: Arc::new(image),
                        position: (0.5, 0.5),
                        size: 0.25,
                    });
                    this.selected = Some(Selection::Sticker(this.stickers.len() - 1));
                    this.browsing_stickers = false;
                    this.status = format!("Added {title} from Wikimedia Commons");
                    this.refresh(cx);
                }
                Err(error) => {
                    this.status = format!("Could not download sticker: {error}");
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn copy_image(&mut self, cx: &mut Context<Self>) {
        let Some(image) = self.image.clone() else {
            self.status = "Choose an image first.".into();
            cx.notify();
            return;
        };
        let captions: Vec<_> = self.captions.iter().map(|c| c.data.clone()).collect();
        let stickers = self.stickers.clone();
        self.status = "Preparing image for clipboard…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let image = make_meme(&image, &captions, &stickers)?;
                    let mut bytes = std::io::Cursor::new(Vec::new());
                    image
                        .write_to(&mut bytes, image::ImageFormat::Png)
                        .map_err(|e| e.to_string())?;
                    Ok::<_, String>(bytes.into_inner())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(bytes) => {
                        cx.write_to_clipboard(ClipboardItem::new_image(
                            &gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, bytes),
                        ));
                        this.status = "Image copied to clipboard".into();
                    }
                    Err(error) => this.status = format!("Could not copy image: {error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn add_sticker(&mut self, cx: &mut Context<Self>) {
        self.pick_image(true, cx);
    }
    fn pick_image(&mut self, sticker: bool, cx: &mut Context<Self>) {
        let dialog = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open image".into()),
        });
        cx.spawn(async move |this, cx| {
            let path = match dialog.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => path,
                    None => return,
                },
                Ok(Ok(None)) => return,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.status = format!("Could not open file dialog: {error}");
                        cx.notify();
                    });
                    return;
                }
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        this.status = format!("File dialog failed: {error}");
                        cx.notify();
                    });
                    return;
                }
            };
            let result = cx
                .background_spawn(async move {
                    image::open(&path)
                        .map(|image| (path, image))
                        .map_err(|e| e.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok((path, image)) if sticker => {
                    this.stickers.push(Sticker {
                        image: Arc::new(image),
                        position: (0.5, 0.5),
                        size: 0.25,
                    });
                    this.selected = Some(Selection::Sticker(this.stickers.len() - 1));
                    this.status = format!(
                        "Added {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                    this.refresh(cx);
                }
                Ok((path, image)) => {
                    this.request_id += 1;
                    this.loading = false;
                    this.browsing = false;
                    this.browsing_stickers = false;
                    this.status = format!(
                        "Loaded {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                    this.preview_base = Some(Arc::new(image.thumbnail(640, 640)));
                    this.image = Some(image);
                    this.refresh(cx);
                }
                Err(error) => {
                    this.status = format!("Could not open image: {error}");
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn resize_selection(&mut self, direction: f32) {
        match self.selected {
            Some(Selection::Caption(id)) => {
                if let Some(c) = self.captions.iter_mut().find(|c| c.id == id) {
                    c.data.size = (c.data.size + direction * 5.).clamp(20., 160.);
                }
            }
            Some(Selection::Sticker(index)) => {
                if let Some(s) = self.stickers.get_mut(index) {
                    s.size = (s.size + direction * 0.05).clamp(0.05, 2.);
                }
            }
            None => {}
        }
    }
    fn delete_selection(&mut self) {
        match self.selected.take() {
            Some(Selection::Caption(id)) => self.captions.retain(|c| c.id != id),
            Some(Selection::Sticker(index)) => {
                if index < self.stickers.len() {
                    self.stickers.remove(index);
                }
            }
            None => {}
        }
        self.drag = None;
    }
    fn browse(&mut self, cx: &mut Context<Self>) {
        self.browsing_stickers = false;
        self.browsing = true;
        if !self.templates.is_empty() || self.loading {
            cx.notify();
            return;
        }
        self.loading = true;
        self.status = "Loading templates from Imgflip…".into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { fetch_catalog() }).await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(templates) => {
                        this.status = format!("{} templates from Imgflip", templates.len());
                        this.templates = templates;
                    }
                    Err(error) => this.status = format!("Could not load templates: {error}"),
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn select_template(&mut self, template: Template, cx: &mut Context<Self>) {
        self.request_id += 1;
        let request_id = self.request_id;
        self.loading = true;
        self.status = format!("Downloading {}…", template.name);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let result = fetch_template(&template.url);
                    (template.name, result)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.request_id != request_id {
                    return;
                }
                this.loading = false;
                match result {
                    (name, Ok(image)) => {
                        this.preview_base = Some(Arc::new(image.thumbnail(640, 640)));
                        this.image = Some(image);
                        this.browsing = false;
                        this.status = format!("Loaded {name} from Imgflip");
                        this.refresh(cx);
                    }
                    (_, Err(error)) => {
                        this.status = format!("Could not download template: {error}");
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.preview_generation += 1;
        if self.preview_busy {
            return;
        }
        self.render_preview(cx);
    }
    fn render_preview(&mut self, cx: &mut Context<Self>) {
        let Some(image) = self.preview_base.clone() else {
            cx.notify();
            return;
        };
        self.preview_busy = true;
        let generation = self.preview_generation;
        let captions: Vec<_> = self.captions.iter().map(|c| c.data.clone()).collect();
        let stickers = self.stickers.clone();
        let rendered_base = image.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    make_meme(&image, &captions, &stickers)
                        .map(|result| preview_pixels(result.to_rgba8()))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.preview_busy = false;
                // Show completed frames during continuous input, then render the latest state.
                // Do not show a frame from an image that has since been replaced.
                if this
                    .preview_base
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &rendered_base))
                {
                    match result {
                        Ok(image) => {
                            this.preview =
                                Some(Arc::new(RenderImage::new(vec![image::Frame::new(image)])))
                        }
                        Err(err) => this.status = err,
                    }
                    cx.notify();
                }
                if generation != this.preview_generation {
                    this.render_preview(cx);
                }
            });
        })
        .detach();
    }
    fn open(&mut self, cx: &mut Context<Self>) {
        self.pick_image(false, cx);
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.image.clone() else {
            self.status = "Choose an image first.".into();
            cx.notify();
            return;
        };
        let captions: Vec<_> = self.captions.iter().map(|c| c.data.clone()).collect();
        let stickers = self.stickers.clone();
        let directory = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let dialog = cx.prompt_for_new_path(&directory, Some("meme.png"));
        cx.spawn(async move |this, cx| {
            let mut path = match dialog.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) => return,
                Ok(Err(error)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.status = format!("Could not open save dialog: {error}");
                        cx.notify();
                    });
                    return;
                }
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        this.status = format!("Save dialog failed: {error}");
                        cx.notify();
                    });
                    return;
                }
            };
            if path.extension().is_none() {
                path.set_extension("png");
                if path.exists() {
                    let _ = this.update(cx, |this, cx| {
                        this.status = format!(
                            "File already exists: {}. Choose that filename to replace it.",
                            path.display()
                        );
                        cx.notify();
                    });
                    return;
                }
            } else if !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
            {
                let _ = this.update(cx, |this, cx| {
                    this.status = "Choose a filename ending in .png".into();
                    cx.notify();
                });
                return;
            }
            let result = cx
                .background_spawn(async move {
                    make_meme(&source, &captions, &stickers)
                        .and_then(|img| {
                            img.save_with_format(&path, image::ImageFormat::Png)
                                .map_err(|e| e.to_string())
                        })
                        .map(|()| path)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.status = match result {
                    Ok(path) => format!("Saved to {}", path.display()),
                    Err(err) => format!("Could not save: {err}"),
                };
                cx.notify();
            });
        })
        .detach();
    }
}

fn button(
    id: &'static str,
    label: &'static str,
    accent: bool,
    action: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .px_4()
        .py_3()
        .rounded_lg()
        .cursor_pointer()
        .text_sm()
        .font_weight(FontWeight::SEMIBOLD)
        .bg(if accent { rgb(0xc9f36b) } else { rgb(0x292d35) })
        .text_color(if accent { rgb(0x17210a) } else { rgb(0xe9eaf0) })
        .hover(|s| s.opacity(0.85))
        .on_click(action)
        .child(label)
}

impl Render for MemeApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let caption_controls = self
            .captions
            .iter()
            .enumerate()
            .map(|(index, caption)| {
                let id = caption.id;
                div()
                    .id(("caption", id))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(if self.selected == Some(Selection::Caption(id)) {
                        rgb(0xc9f36b)
                    } else {
                        rgb(0x3a3f49)
                    })
                    .child(
                        div()
                            .id(("select-caption", id))
                            .cursor_pointer()
                            .text_xs()
                            .text_color(rgb(0xc3c6ce))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected = Some(Selection::Caption(id));
                                cx.notify();
                            }))
                            .child(format!(
                                "TEXT {}{}",
                                index + 1,
                                if self.selected == Some(Selection::Caption(id)) {
                                    "  • SELECTED"
                                } else {
                                    ""
                                }
                            )),
                    )
                    .child(Input::new(&caption.input))
            })
            .collect::<Vec<_>>();
        let selection_size = match self.selected {
            Some(Selection::Caption(id)) => self
                .captions
                .iter()
                .find(|c| c.id == id)
                .map(|c| format!("{}", c.data.size as u32)),
            Some(Selection::Sticker(index)) => self
                .stickers
                .get(index)
                .map(|s| format!("{}%", (s.size * 100.) as u32)),
            None => None,
        };
        let preview = if self.browsing_stickers {
            let results = self
                .sticker_results
                .iter()
                .enumerate()
                .map(|(index, result)| {
                    let item = result.clone();
                    let source = result.source_url.clone();
                    let thumb = if let Some(image) = &result.thumbnail {
                        img(image.clone())
                            .size(px(80.))
                            .object_fit(ObjectFit::Contain)
                            .into_any_element()
                    } else {
                        div().size(px(80.)).bg(rgb(0x343943)).into_any_element()
                    };
                    div()
                        .flex()
                        .gap_3()
                        .p_2()
                        .bg(rgb(0x292d35))
                        .rounded_lg()
                        .items_center()
                        .child(thumb)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(result.title.clone())
                                .child(
                                    div()
                                        .id(("sticker-source", index))
                                        .text_xs()
                                        .text_color(rgb(0xc9f36b))
                                        .cursor_pointer()
                                        .on_click(move |_, _, cx| cx.open_url(&source))
                                        .child("Source and license ↗"),
                                ),
                        )
                        .child(
                            div()
                                .id(("sticker-add", index))
                                .cursor_pointer()
                                .px_3()
                                .py_2()
                                .rounded_lg()
                                .bg(rgb(0x444b55))
                                .hover(|s| s.opacity(0.8))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.use_online_sticker(item.clone(), cx)
                                }))
                                .child("Add"),
                        )
                })
                .collect::<Vec<_>>();
            div()
                .size_full()
                .p_6()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .child("Find stickers"),
                        )
                        .child(button(
                            "close-stickers",
                            "Close",
                            false,
                            cx.listener(|this, _, _, cx| {
                                this.browsing_stickers = false;
                                cx.notify();
                            }),
                        )),
                )
                .child(div().text_sm().text_color(rgb(0x959ba8)).child(
                    "Wikimedia Commons · Images may not be transparent; check each source license",
                ))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(div().flex_1().child(Input::new(&self.sticker_input)))
                        .child(button(
                            "search-stickers",
                            "Search",
                            false,
                            cx.listener(|this, _, _, cx| this.search_stickers(cx)),
                        )),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0x959ba8))
                        .child(self.status.clone()),
                )
                .child(
                    div()
                        .id("sticker-results")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(results),
                )
                .into_any_element()
        } else if self.browsing {
            let query = self.search.to_lowercase();
            let cards = self
                .templates
                .iter()
                .filter(|t| t.name.to_lowercase().contains(&query))
                .enumerate()
                .map(|(index, template)| {
                    let chosen = template.clone();
                    div()
                        .id(("template", index))
                        .w_full()
                        .px_4()
                        .py_3()
                        .rounded_lg()
                        .bg(rgb(0x292d35))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(0x3a4049)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_template(chosen.clone(), cx)
                        }))
                        .child(template.name.clone())
                })
                .collect::<Vec<_>>();
            div()
                .size_full()
                .p_6()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .child("Templates"),
                        )
                        .child(button(
                            "close-catalog",
                            "Close",
                            false,
                            cx.listener(|this, _, _, cx| {
                                this.browsing = false;
                                cx.notify();
                            }),
                        )),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0x959ba8))
                        .child("Popular blank templates from Imgflip"),
                )
                .child(Input::new(&self.search_input))
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(0x959ba8))
                        .child(self.status.clone()),
                )
                .child(
                    div()
                        .id("templates-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(cards),
                )
                .into_any_element()
        } else if let Some(image) = &self.preview {
            let bounds = self.preview_bounds.clone();
            div()
                .id("draggable-preview")
                .relative()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        this.begin_drag(event);
                        cx.notify();
                    }),
                )
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    if let Some((selection, start, offset, w, h)) = this.drag {
                        let position = (
                            (offset.0 + f32::from(event.position.x - start.x) / w).clamp(-1., 1.),
                            (offset.1 + f32::from(event.position.y - start.y) / h).clamp(-1., 1.),
                        );
                        match selection {
                            Selection::Caption(id) => {
                                if let Some(caption) = this.captions.iter_mut().find(|c| c.id == id)
                                {
                                    caption.data.offset = position;
                                }
                            }
                            Selection::Sticker(index) => {
                                if let Some(sticker) = this.stickers.get_mut(index) {
                                    sticker.position = position;
                                }
                            }
                        }
                        this.refresh(cx);
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| this.drag = None),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| this.drag = None),
                )
                .child(
                    canvas(
                        move |rect, _, _| {
                            bounds.set(Some(rect));
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0()
                    .size_full(),
                )
                .child(
                    img(image.clone())
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0x858c99))
                        .child("No image selected"),
                )
                .child(button(
                    "empty-open",
                    "Choose image",
                    false,
                    cx.listener(|this, _, _, cx| this.open(cx)),
                ))
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x171a20))
            .text_color(rgb(0xf4f5f7))
            .font_family("Helvetica Neue")
            .child(
                div()
                    .h(px(76.))
                    .flex_shrink_0()
                    .px_6()
                    .border_b_1()
                    .border_color(rgb(0x30343d))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().font_weight(FontWeight::BOLD).child("Meemuttaja"))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(button(
                                "copy-image",
                                "Copy image",
                                false,
                                cx.listener(|this, _, _, cx| this.copy_image(cx)),
                            ))
                            .child(button(
                                "export",
                                "Export PNG",
                                true,
                                cx.listener(|this, _, _, cx| this.save(cx)),
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("controls-panel")
                            .w(px(320.))
                            .flex_shrink_0()
                            .overflow_y_scroll()
                            .h_full()
                            .p_6()
                            .border_r_1()
                            .border_color(rgb(0x30343d))
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0x959ba8))
                                    .child("IMAGE"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        div().flex_1().child(
                                            button(
                                                "open",
                                                "Open file",
                                                false,
                                                cx.listener(|this, _, _, cx| this.open(cx)),
                                            )
                                            .w_full(),
                                        ),
                                    )
                                    .child(
                                        div().flex_1().child(
                                            button(
                                                "browse",
                                                "Templates",
                                                false,
                                                cx.listener(|this, _, _, cx| this.browse(cx)),
                                            )
                                            .w_full(),
                                        ),
                                    ),
                            )
                            .child(div().h(px(1.)).bg(rgb(0x383d46)))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(0x959ba8))
                                            .child("LAYERS"),
                                    )
                                    .child(button(
                                        "add-text",
                                        "+ Text",
                                        false,
                                        cx.listener(|this, _, window, cx| {
                                            this.add_caption(Anchor::Center, window, cx)
                                        }),
                                    )),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x858c99))
                                    .child("Select a layer to edit it; drag it on the image."),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        div().flex_1().child(
                                            button(
                                                "find-stickers",
                                                "Find stickers",
                                                false,
                                                cx.listener(|this, _, _, cx| {
                                                    this.browsing = false;
                                                    this.browsing_stickers = true;
                                                    this.status =
                                                        "Search Wikimedia Commons for images"
                                                            .into();
                                                    cx.notify();
                                                }),
                                            )
                                            .w_full(),
                                        ),
                                    )
                                    .child(
                                        div().flex_1().child(
                                            button(
                                                "add-sticker",
                                                "Open sticker",
                                                false,
                                                cx.listener(|this, _, _, cx| this.add_sticker(cx)),
                                            )
                                            .w_full(),
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .id("caption-list")
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .children(caption_controls)
                                    .children(self.stickers.iter().enumerate().map(
                                        |(index, _)| {
                                            div()
                                                .id(("select-sticker", index))
                                                .p_2()
                                                .rounded_lg()
                                                .cursor_pointer()
                                                .border_1()
                                                .border_color(
                                                    if self.selected
                                                        == Some(Selection::Sticker(index))
                                                    {
                                                        rgb(0xc9f36b)
                                                    } else {
                                                        rgb(0x3a3f49)
                                                    },
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.selected = Some(Selection::Sticker(index));
                                                    cx.notify();
                                                }))
                                                .child(format!(
                                                    "Sticker {}{}",
                                                    index + 1,
                                                    if self.selected
                                                        == Some(Selection::Sticker(index))
                                                    {
                                                        " • selected"
                                                    } else {
                                                        ""
                                                    }
                                                ))
                                        },
                                    )),
                            )
                            .child(div().h(px(1.)).bg(rgb(0x383d46)))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0x959ba8))
                                    .child("SELECTED LAYER"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child("Size")
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_3()
                                            .child(button(
                                                "smaller",
                                                "−",
                                                false,
                                                cx.listener(|this, _, _, cx| {
                                                    this.resize_selection(-1.);
                                                    this.refresh(cx);
                                                }),
                                            ))
                                            .child(selection_size.unwrap_or_else(|| "—".into()))
                                            .child(button(
                                                "larger",
                                                "+",
                                                false,
                                                cx.listener(|this, _, _, cx| {
                                                    this.resize_selection(1.);
                                                    this.refresh(cx);
                                                }),
                                            )),
                                    ),
                            )
                            .child(button(
                                "delete-selected",
                                "Delete layer",
                                false,
                                cx.listener(|this, _, _, cx| {
                                    this.delete_selection();
                                    this.refresh(cx);
                                }),
                            ))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x858c99))
                                    .child(self.status.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .p_8()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(0x858c99))
                                    .child("PREVIEW"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_h_0()
                                    .rounded_xl()
                                    .bg(rgb(0x22262d))
                                    .border_1()
                                    .border_color(rgb(0x343943))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .overflow_hidden()
                                    .child(preview),
                            ),
                    ),
            )
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            let bounds = Bounds::centered(None, size(px(1100.), px(760.)), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    icon: image::load_from_memory(include_bytes!("../assets/icon.png"))
                        .ok()
                        .map(|icon| Arc::new(icon.thumbnail(256, 256).to_rgba8())),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let search_input = cx
                            .new(|cx| InputState::new(window, cx).placeholder("Search templates"));
                        let mut subscriptions: Vec<Subscription> = Vec::new();
                        let sticker_input =
                            cx.new(|cx| InputState::new(window, cx).placeholder("Search images"));
                        let sticker_search = sticker_input.clone();
                        subscriptions.push(cx.subscribe_in(
                            &sticker_input,
                            window,
                            move |this: &mut MemeApp, _, event: &InputEvent, _, cx| {
                                if matches!(event, InputEvent::Change) {
                                    this.sticker_query =
                                        sticker_search.read(cx).value().to_string();
                                    cx.notify();
                                }
                            },
                        ));
                        let search = search_input.clone();
                        subscriptions.push(cx.subscribe_in(
                            &search_input,
                            window,
                            move |this: &mut MemeApp, _, event: &InputEvent, _, cx| {
                                if matches!(event, InputEvent::Change) {
                                    this.search = search.read(cx).value().to_string();
                                    cx.notify();
                                }
                            },
                        ));
                        let mut app = MemeApp {
                            image: None,
                            templates: Vec::new(),
                            browsing: false,
                            browsing_stickers: false,
                            sticker_query: String::new(),
                            sticker_input,
                            sticker_results: Vec::new(),
                            sticker_search_id: 0,
                            sticker_loading: false,
                            loading: false,
                            request_id: 0,
                            search_input,
                            search: String::new(),
                            preview: None,
                            preview_base: None,
                            preview_generation: 0,
                            preview_busy: false,
                            captions: Vec::new(),
                            next_caption_id: 0,
                            selected: None,
                            stickers: Vec::new(),
                            _subscriptions: subscriptions,
                            status: "PNG, JPG, WebP, GIF or BMP".into(),
                            drag: None,
                            preview_bounds: Rc::new(Cell::new(None)),
                        };
                        app.add_caption(Anchor::Top, window, cx);
                        app.add_caption(Anchor::Bottom, window, cx);
                        app.selected = Some(Selection::Caption(0));
                        app
                    })
                },
            )
            .expect("open window");
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use super::{make_meme, preview_pixels, Sticker};
    use image::Rgba;
    use std::sync::Arc;

    #[test]
    fn preview_converts_rgba_to_bgra() {
        let image = image::RgbaImage::from_pixel(1, 1, Rgba([240, 120, 30, 200]));
        assert_eq!(preview_pixels(image).get_pixel(0, 0).0, [30, 120, 240, 200]);
    }
    #[test]
    fn sticker_overlay_keeps_transparency_and_position() {
        let source = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            100,
            100,
            Rgba([0, 0, 255, 255]),
        ));
        let mut pixels = image::RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 0]));
        for y in 5..15 {
            for x in 5..15 {
                pixels.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        let sticker = Sticker {
            image: Arc::new(image::DynamicImage::ImageRgba8(pixels)),
            position: (0.5, 0.5),
            size: 0.2,
        };
        let result = make_meme(&source, &[], &[sticker]).unwrap().to_rgba8();
        assert_eq!(result.get_pixel(50, 50).0, [255, 0, 0, 255]);
        assert_eq!(result.get_pixel(40, 40).0, [0, 0, 255, 255]);
    }
}
