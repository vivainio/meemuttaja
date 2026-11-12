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

struct MemeApp {
    image: Option<DynamicImage>,
    templates: Vec<Template>,
    browsing: bool,
    loading: bool,
    request_id: u64,
    search_input: Entity<InputState>,
    search: String,
    preview: Option<Arc<RenderImage>>,
    preview_base: Option<Arc<DynamicImage>>,
    preview_generation: u64,
    preview_busy: bool,
    top: String,
    bottom: String,
    top_input: Entity<InputState>,
    bottom_input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
    font_size: f32,
    status: String,
    caption_offsets: [(f32, f32); 2],
    drag: Option<(usize, Point<Pixels>, (f32, f32), f32, f32)>,
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
    top: &str,
    bottom: &str,
    size: f32,
    offsets: [(f32, f32); 2],
) -> Result<DynamicImage, String> {
    let font = font().ok_or("Could not find a bold system font")?;
    let mut out = source.to_rgba8();
    let (w, h) = out.dimensions();
    let scale = PxScale::from((w as f32 * size / 1000.0).max(8.0));
    let scaled = font.as_scaled(scale);
    let line_height = (scaled.height() * 1.1).ceil() as i32;
    let margin = (w as f32 * 0.025) as i32;
    let bottom_margin = ((h as f32 * 0.08) as i32).max(margin);
    for (index, (text, is_bottom)) in [(top, false), (bottom, true)].into_iter().enumerate() {
        let lines = wrap_caption(text, &font, scale, w, margin);
        let start_y = if is_bottom {
            h as i32 - bottom_margin - line_height * lines.len() as i32
        } else {
            margin
        };
        let start_y = start_y + (offsets[index].1 * h as f32).round() as i32;
        for (i, line) in lines.iter().enumerate() {
            let tw = imageproc::drawing::text_size(scale, &font, line).0 as i32;
            let x = (w as i32 - tw) / 2 + (offsets[index].0 * w as f32).round() as i32;
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
        let scale = PxScale::from((iw * self.font_size / 1000.).max(8.));
        let line_height = (font.as_scaled(scale).height() * 1.1).ceil();
        let margin = iw * 0.025;
        // Measure the same wrapped lines that are drawn by make_meme.
        for (index, text) in [&self.top, &self.bottom].into_iter().enumerate().rev() {
            if text.trim().is_empty() {
                continue;
            }
            let lines = wrap_caption(text, &font, scale, image.width(), margin as i32);
            let block_height = line_height * lines.len() as f32;
            let base_y = if index == 0 {
                margin
            } else {
                ih - (ih * 0.08).max(margin) - block_height
            };
            let offset = self.caption_offsets[index];
            let left = lines
                .iter()
                .map(|line| imageproc::drawing::text_size(scale, &font, line).0 as f32)
                .fold(0., f32::max);
            let image_x = px / w * iw;
            let image_y = py / h * ih;
            if image_x >= (iw - left) / 2. + offset.0 * iw - margin
                && image_x <= (iw + left) / 2. + offset.0 * iw + margin
                && image_y >= base_y + offset.1 * ih - margin
                && image_y <= base_y + offset.1 * ih + block_height + margin
            {
                self.drag = Some((index, event.position, offset, w, h));
                break;
            }
        }
    }
    fn browse(&mut self, cx: &mut Context<Self>) {
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
        let top = self.top.clone();
        let bottom = self.bottom.clone();
        let font_size = self.font_size;
        let offsets = self.caption_offsets;
        let rendered_base = image.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    make_meme(&image, &top, &bottom, font_size, offsets)
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
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
            .pick_file()
        {
            match image::open(&path) {
                Ok(image) => {
                    self.request_id += 1;
                    self.loading = false;
                    self.browsing = false;
                    self.status = format!(
                        "Loaded {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                    self.preview_base = Some(Arc::new(image.thumbnail(640, 640)));
                    self.image = Some(image);
                    self.refresh(cx);
                }
                Err(err) => {
                    self.status = format!("Could not open image: {err}");
                    cx.notify();
                }
            }
        }
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(source) = &self.image else {
            self.status = "Choose an image first.".into();
            cx.notify();
            return;
        };
        if let Some(mut path) = rfd::FileDialog::new()
            .set_file_name("meme.png")
            .add_filter("PNG image", &["png"])
            .save_file()
        {
            path.set_extension("png");
            self.status = match make_meme(
                source,
                &self.top,
                &self.bottom,
                self.font_size,
                self.caption_offsets,
            )
            .and_then(|img| img.save(&path).map_err(|e| e.to_string()))
            {
                Ok(()) => format!("Saved to {}", path.display()),
                Err(err) => format!("Could not save: {err}"),
            };
            cx.notify();
        }
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
        let field = |label: &'static str, input: &Entity<InputState>| {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(0x959ba8))
                        .child(label),
                )
                .child(Input::new(input))
        };
        let preview = if self.browsing {
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
                    cx.listener(|this, event: &MouseDownEvent, _, _| this.begin_drag(event)),
                )
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    if let Some((index, start, offset, w, h)) = this.drag {
                        this.caption_offsets[index] = (
                            (offset.0 + f32::from(event.position.x - start.x) / w).clamp(-1., 1.),
                            (offset.1 + f32::from(event.position.y - start.y) / h).clamp(-1., 1.),
                        );
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
            .bg(rgb(0x171a20))
            .text_color(rgb(0xf4f5f7))
            .font_family("Helvetica Neue")
            .child(
                div()
                    .h(px(76.))
                    .px_6()
                    .border_b_1()
                    .border_color(rgb(0x30343d))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().font_weight(FontWeight::BOLD).child("Meemuttaja"))
                    .child(button(
                        "export",
                        "Export PNG",
                        true,
                        cx.listener(|this, _, _, cx| this.save(cx)),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .h_full()
                    .child(
                        div()
                            .w(px(320.))
                            .h_full()
                            .p_6()
                            .border_r_1()
                            .border_color(rgb(0x30343d))
                            .flex()
                            .flex_col()
                            .gap_5()
                            .child(button(
                                "open",
                                "Choose image",
                                false,
                                cx.listener(|this, _, _, cx| this.open(cx)),
                            ))
                            .child(button(
                                "browse",
                                "Browse templates",
                                false,
                                cx.listener(|this, _, _, cx| this.browse(cx)),
                            ))
                            .child(div().h(px(1.)).bg(rgb(0x383d46)))
                            .child(field("TOP TEXT", &self.top_input))
                            .child(field("BOTTOM TEXT", &self.bottom_input))
                            .child(button(
                                "reset-positions",
                                "Reset text positions",
                                false,
                                cx.listener(|this, _, _, cx| {
                                    this.caption_offsets = [(0., 0.); 2];
                                    this.refresh(cx);
                                }),
                            ))
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
                                            .child("TEXT SIZE"),
                                    )
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
                                                    this.font_size = (this.font_size - 5.).max(20.);
                                                    this.refresh(cx);
                                                }),
                                            ))
                                            .child(format!("{}", self.font_size as u32))
                                            .child(button(
                                                "larger",
                                                "+",
                                                false,
                                                cx.listener(|this, _, _, cx| {
                                                    this.font_size =
                                                        (this.font_size + 5.).min(160.);
                                                    this.refresh(cx);
                                                }),
                                            )),
                                    ),
                            )
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
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let search_input = cx
                            .new(|cx| InputState::new(window, cx).placeholder("Search templates"));
                        let top_input =
                            cx.new(|cx| InputState::new(window, cx).placeholder("Add top text"));
                        let bottom_input =
                            cx.new(|cx| InputState::new(window, cx).placeholder("Add bottom text"));
                        let mut subscriptions: Vec<Subscription> = [&top_input, &bottom_input]
                            .iter()
                            .enumerate()
                            .map(|(index, input)| {
                                let input = (*input).clone();
                                cx.subscribe_in(
                                    &input.clone(),
                                    window,
                                    move |this: &mut MemeApp, _, event: &InputEvent, _, cx| {
                                        if matches!(event, InputEvent::Change) {
                                            let value = input.read(cx).value().to_string();
                                            if index == 0 {
                                                this.top = value;
                                            } else {
                                                this.bottom = value;
                                            }
                                            this.refresh(cx);
                                        }
                                    },
                                )
                            })
                            .collect();
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
                        MemeApp {
                            image: None,
                            templates: Vec::new(),
                            browsing: false,
                            loading: false,
                            request_id: 0,
                            search_input,
                            search: String::new(),
                            preview: None,
                            preview_base: None,
                            preview_generation: 0,
                            preview_busy: false,
                            top: String::new(),
                            bottom: String::new(),
                            top_input,
                            bottom_input,
                            _subscriptions: subscriptions,
                            font_size: 65.,
                            status: "PNG, JPG, WebP, GIF or BMP".into(),
                            caption_offsets: [(0., 0.); 2],
                            drag: None,
                            preview_bounds: Rc::new(Cell::new(None)),
                        }
                    })
                },
            )
            .expect("open window");
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use super::preview_pixels;
    use image::Rgba;

    #[test]
    fn preview_converts_rgba_to_bgra() {
        let image = image::RgbaImage::from_pixel(1, 1, Rgba([240, 120, 30, 200]));
        assert_eq!(preview_pixels(image).get_pixel(0, 0).0, [30, 120, 240, 200]);
    }
}
