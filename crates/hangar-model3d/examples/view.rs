//! Opens a model in a window with the viewer:
//!
//! ```text
//! cargo run -p hangar-model3d --example view -- <file>
//! cargo run -p hangar-model3d --example view            # a generated sample
//! cargo run -p hangar-model3d --example view -- --sample obj|stl|ply|gltf|glb|textured|cloud
//! ```
//!
//! Left button turns, right button or Shift+left pans, the wheel zooms toward
//! the pointer; click the view, then the arrow keys, + and -, and Home work
//! too.
//!
//! For a look without a window: `--shot out.png [--angle iso]` writes the
//! picture an agent's snapshot would be and exits; `--window-shot out.png`
//! opens the window, waits for the model, saves what the window drew and
//! exits (that is how the widget is checked without a person at the screen).

use std::path::{Path, PathBuf};

use hangar_model3d::load::{self, LoadOptions};
use hangar_model3d::{
    CaptureError, CaptureJob, DisplayOptions, Event, LoadEvent, LoadState, ModelViewer,
    SnapshotAngle, ViewerTheme, describe, samples,
};
use iced::widget::{button, checkbox, column, container, row, text};
use iced::window::{self, Screenshot};
use iced::{Element, Length, Subscription, Task, Theme, time};

#[derive(Debug, Clone)]
enum Message {
    Viewer(Event),
    Load(LoadEvent),
    Wireframe(bool),
    Grid(bool),
    Axes(bool),
    Normals(bool),
    Recenter,
    Light(bool),
    Capture,
    Captured(Result<Vec<u8>, CaptureError>),
    Tick,
    Shot(Screenshot),
}

struct Args {
    path: PathBuf,
    shot: Option<PathBuf>,
    window_shot: Option<PathBuf>,
    angle: SnapshotAngle,
    size: (u32, u32),
    display: DisplayOptions,
    light: bool,
}

struct App {
    args: Args,
    viewer: ModelViewer,
    status: String,
    ticks: u32,
    shot_requested: bool,
}

fn sample(kind: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("hangar-model3d-samples");
    std::fs::create_dir_all(&dir).expect("a folder for the samples");
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("a sample is written");
        path
    };
    match kind {
        "obj" => {
            let (obj, mtl) = samples::cube_obj();
            write("cube.mtl", mtl.as_bytes());
            write("cube.obj", obj.as_bytes())
        }
        "stl" => write("cube.stl", &samples::cube_stl_binary()),
        "ply" => write("cube.ply", samples::cube_ply_ascii().as_bytes()),
        "cloud" => write("cloud.ply", samples::point_cloud_ply(20_000).as_bytes()),
        "gltf" => write("cube.gltf", samples::cube_gltf_with_data_uris().as_bytes()),
        "glb" => write("cube.glb", &samples::cube_glb()),
        _ => write("textured.glb", &samples::textured_cube_glb()),
    }
}

fn parse() -> Args {
    let mut args = Args {
        path: sample("textured"),
        shot: None,
        window_shot: None,
        angle: SnapshotAngle::Iso,
        size: (800, 600),
        display: DisplayOptions::default(),
        light: false,
    };
    let mut input = std::env::args().skip(1);
    while let Some(arg) = input.next() {
        match arg.as_str() {
            "--sample" => args.path = sample(&input.next().unwrap_or_default()),
            "--shot" => args.shot = input.next().map(PathBuf::from),
            "--window-shot" => args.window_shot = input.next().map(PathBuf::from),
            "--angle" => {
                args.angle = input
                    .next()
                    .unwrap_or_default()
                    .parse()
                    .unwrap_or_else(|error: String| panic!("{error}"));
            }
            "--size" => {
                let size = input.next().unwrap_or_default();
                let (w, h) = size.split_once('x').expect("--size 800x600");
                args.size = (w.parse().expect("a width"), h.parse().expect("a height"));
            }
            "--wire" => args.display.wireframe = true,
            "--axes" => args.display.axes = true,
            "--normals" => args.display.normals = true,
            "--no-grid" => args.display.grid = false,
            "--light" => args.light = true,
            file => args.path = PathBuf::from(file),
        }
    }
    args
}

fn theme_of(light: bool) -> ViewerTheme {
    if light {
        ViewerTheme::light()
    } else {
        ViewerTheme::dark()
    }
}

fn write_png(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    println!("wrote {}", path.display());
}

impl App {
    fn boot() -> (App, Task<Message>) {
        let args = parse();
        let (events, _cancel) = load::start(args.path.clone(), LoadOptions::default());
        let mut viewer = ModelViewer::new();
        viewer.set_display(args.display);
        let app = App {
            viewer,
            status: format!("Opening {}", args.path.display()),
            args,
            ticks: 0,
            shot_requested: false,
        };
        (app, Task::run(events, Message::Load))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Viewer(_) => {}
            Message::Load(event) => {
                let done = matches!(event, LoadEvent::Done(_));
                self.viewer.apply(event);
                if done {
                    self.status = match self.viewer.state() {
                        LoadState::Failed(error) => error.clone(),
                        _ => self.viewer.model().map_or_else(String::new, |model| {
                            let info = &model.info;
                            format!(
                                "{}: {} triangles, {} vertices, {} meshes, {} materials, {:.3} x {:.3} x {:.3}{}",
                                model.name,
                                info.triangles,
                                info.vertices,
                                info.meshes,
                                info.materials,
                                info.size[0],
                                info.size[1],
                                info.size[2],
                                model.format.plan().unit.map_or(String::new(), |u| format!(" {u}")),
                            )
                        }),
                    };
                    println!("{}", self.status);
                }
            }
            Message::Wireframe(on) => self.toggle(|d| d.wireframe = on),
            Message::Grid(on) => self.toggle(|d| d.grid = on),
            Message::Axes(on) => self.toggle(|d| d.axes = on),
            Message::Normals(on) => self.toggle(|d| d.normals = on),
            Message::Recenter => self.viewer.recenter(),
            Message::Light(on) => self.args.light = on,
            Message::Capture => {
                if let Some(job) = self.viewer.capture(2.0, theme_of(self.args.light)) {
                    return Task::perform(job.run_async(), Message::Captured);
                }
            }
            Message::Captured(result) => match result {
                Ok(png) => {
                    let path = std::env::temp_dir().join("hangar-model3d-capture.png");
                    write_png(&path, &png);
                    self.status = format!("Captured to {}", path.display());
                }
                Err(error) => self.status = error.to_string(),
            },
            Message::Tick => {
                self.ticks += 1;
                let ready = matches!(self.viewer.state(), LoadState::Ready);
                if let (Some(_), true, true, false) = (
                    &self.args.window_shot,
                    ready,
                    self.ticks > 12,
                    self.shot_requested,
                ) {
                    self.shot_requested = true;
                    return window::latest()
                        .and_then(window::screenshot)
                        .map(Message::Shot);
                }
            }
            Message::Shot(screenshot) => {
                if let Some(path) = &self.args.window_shot {
                    let size = screenshot.size;
                    let image = image::RgbaImage::from_raw(
                        size.width,
                        size.height,
                        screenshot.rgba.to_vec(),
                    )
                    .expect("a screenshot is width x height x 4 bytes");
                    image.save(path).expect("the screenshot is saved");
                    println!("wrote {} ({}x{})", path.display(), size.width, size.height);
                }
                return iced::exit();
            }
        }
        Task::none()
    }

    fn toggle(&mut self, change: impl FnOnce(&mut DisplayOptions)) {
        let mut display = self.viewer.display();
        change(&mut display);
        self.viewer.set_display(display);
    }

    fn view(&self) -> Element<'_, Message> {
        let display = self.viewer.display();
        let bar = row![
            checkbox(display.wireframe)
                .label("Wireframe")
                .on_toggle(Message::Wireframe),
            checkbox(display.grid)
                .label("Grid")
                .on_toggle(Message::Grid),
            checkbox(display.axes)
                .label("Axes")
                .on_toggle(Message::Axes),
            checkbox(display.normals)
                .label("Normals")
                .on_toggle_maybe(self.viewer.can_draw_normals().then_some(Message::Normals)),
            checkbox(self.args.light)
                .label("Light")
                .on_toggle(Message::Light),
            button("Recenter").on_press(Message::Recenter),
            button("Capture").on_press(Message::Capture),
        ]
        .spacing(16)
        .padding(8);
        let stage: Element<'_, Message> = match self.viewer.state() {
            LoadState::Failed(error) => container(text(error.clone())).center(Length::Fill).into(),
            LoadState::Loading(progress) if self.viewer.model().is_none() => {
                container(text(describe(progress)))
                    .center(Length::Fill)
                    .into()
            }
            _ => self
                .viewer
                .view(theme_of(self.args.light))
                .map(Message::Viewer),
        };
        column![
            bar,
            stage,
            container(text(&self.status).size(13)).padding(8)
        ]
        .into()
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.args.window_shot.is_some() {
            time::every(time::Duration::from_millis(50)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    fn theme(&self) -> Theme {
        if self.args.light {
            Theme::Light
        } else {
            Theme::Dark
        }
    }
}

/// `--shot`: loads, takes the picture an agent's snapshot would be, writes it,
/// and never opens a window.
fn shot(args: &Args, path: &Path) {
    let model = match load::load(&args.path) {
        Ok(model) => std::sync::Arc::new(model),
        Err(error) => return eprintln!("{error}"),
    };
    let info = &model.info;
    println!(
        "{}: {} triangles, {} vertices, {} meshes, {} materials, size {:?}",
        model.name, info.triangles, info.vertices, info.meshes, info.materials, info.size
    );
    for warning in &model.warnings {
        println!("warning: {warning}");
    }
    let (w, h) = args.size;
    let scene = if args.display == DisplayOptions::default() {
        CaptureJob::snapshot(model, args.angle, w, h, theme_of(args.light))
    } else {
        let camera = hangar_model3d::camera::Camera::framed(&model, f64::from(w) / f64::from(h));
        CaptureJob::from_camera(model, camera, args.display, w, h, theme_of(args.light))
    };
    match scene.run() {
        Ok(png) => write_png(path, &png),
        Err(error) => eprintln!("{error}"),
    }
}

fn main() -> iced::Result {
    let args = parse();
    if let Some(path) = &args.shot {
        shot(&args, path);
        return Ok(());
    }
    iced::application(App::boot, App::update, App::view)
        .title("hangar-model3d")
        .theme(App::theme)
        .subscription(App::subscription)
        .window_size((1000.0, 720.0))
        .run()
}
