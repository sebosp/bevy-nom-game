use bevy::prelude::*;
use nom_game::NomGamePlugin;

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(code) = cli::run() {
        std::process::exit(code);
    }
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Nom Nom Bytes".into(),
            // The app id of Wayland and the class of X11.
            name: Some("bevy-nom-game".into()),
            resolution: (1440, 900).into(),
            // The web build fills the page, see index.html.
            canvas: Some("#bevy-nom-game".into()),
            fit_canvas_to_parent: true,
            ..default()
        }),
        ..default()
    }))
    .add_plugins(NomGamePlugin);
    #[cfg(not(target_arch = "wasm32"))]
    snapshot::add(&mut app);
    app.run();
}

/// `bevy-nom-game --trace parser.rs file`: the trace of a parser in the terminal, in the colors
/// of s2protocol's `nom_color_debug`.
#[cfg(not(target_arch = "wasm32"))]
mod cli {
    pub fn run() -> Option<i32> {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) != Some("--trace") {
            return None;
        }
        let (Some(script), Some(file)) = (args.get(2), args.get(3)) else {
            eprintln!("usage: bevy-nom-game --trace <parser.rs> <file>");
            return Some(2);
        };
        let (src, input) = match (std::fs::read_to_string(script), std::fs::read(file)) {
            (Ok(src), Ok(input)) => (src, input),
            (Err(e), _) | (_, Err(e)) => {
                eprintln!("{e}");
                return Some(1);
            }
        };
        let outcome = nom_game::script::run(&src, &input);
        for line in nom_game::debug::trace(&outcome, &input) {
            println!("{}", nom_game::debug::to_ansi(&line));
        }
        Some(if outcome.result.is_ok() { 0 } else { 1 })
    }
}

/// `NG_SCREENSHOT=out.png` opens a screen, saves a screenshot and exits, to check the screens
/// without a person:
///
/// - `NG_SCREEN`: menu, level or sandbox;
/// - `NG_LEVEL`: the key of the level;
/// - `NG_CODE`: `solution`, `starter`, or the path of a parser to put in the editor;
/// - `NG_FILE`: a file opened in the sandbox;
/// - `NG_HEX=1`: the other view of the file, xxd for text, text for binary;
/// - `NG_SIZE`: the window size, `1440x900` by default.
///
/// Use `XDG_CONFIG_HOME` to keep the saves away from the real ones.
#[cfg(not(target_arch = "wasm32"))]
mod snapshot {
    use bevy::prelude::*;
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    use bevy::window::{PrimaryWindow, WindowResolution};
    use nom_game::files::Incoming;
    use nom_game::levels;
    use nom_game::progress::Progress;
    use nom_game::screens::workbench::{Bench, Current};
    use nom_game::ui::Screen;

    #[derive(Resource)]
    struct Snapshot {
        path: String,
        screen: Screen,
        toggle: bool,
    }

    pub fn add(app: &mut App) {
        let Ok(path) = std::env::var("NG_SCREENSHOT") else {
            return;
        };
        let screen = match std::env::var("NG_SCREEN").unwrap_or_default().as_str() {
            "level" => Screen::Level,
            "sandbox" => Screen::Sandbox,
            _ => Screen::Menu,
        };
        let toggle = std::env::var("NG_HEX").is_ok_and(|v| v == "1");
        app.insert_resource(Snapshot { path, screen, toggle }).add_systems(Startup, setup).add_systems(Update, run);
    }

    fn setup(
        mut window: Single<&mut Window, With<PrimaryWindow>>,
        mut progress: ResMut<Progress>,
        mut current: ResMut<Current>,
        mut incoming: ResMut<Incoming>,
    ) {
        // A window in the background is throttled by the compositor with vsync.
        window.present_mode = bevy::window::PresentMode::AutoNoVsync;
        let size = std::env::var("NG_SIZE").unwrap_or_default();
        if let Some((w, h)) = size.split_once('x').and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))) {
            window.resolution = WindowResolution::new(w, h);
        }
        let key = std::env::var("NG_LEVEL").unwrap_or_else(|_| "hello".into());
        if let Some((i, level)) = levels::get(&key) {
            current.0 = i;
            match std::env::var("NG_CODE").unwrap_or_default().as_str() {
                "" => {}
                "solution" => {
                    progress.code.insert(level.key.into(), level.solution.into());
                }
                "starter" => {
                    progress.code.insert(level.key.into(), level.starter.into());
                }
                path => {
                    if let Ok(src) = std::fs::read_to_string(path) {
                        let key = if std::env::var("NG_SCREEN").is_ok_and(|s| s == "sandbox") { "sandbox" } else { level.key };
                        progress.code.insert(key.into(), src);
                    }
                }
            }
        }
        if let Ok(path) = std::env::var("NG_FILE")
            && let Ok(bytes) = std::fs::read(&path)
        {
            let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            incoming.0.push((name, bytes));
        }
    }

    fn run(
        mut frame: Local<u32>,
        snapshot: Res<Snapshot>,
        mut next: ResMut<NextState<Screen>>,
        bench: Option<ResMut<Bench>>,
        mut commands: Commands,
        mut exit: MessageWriter<AppExit>,
    ) {
        *frame += 1;
        if *frame == 5 {
            next.set(snapshot.screen);
        }
        if *frame == 20
            && snapshot.toggle
            && let Some(mut bench) = bench
        {
            bench.hex = !bench.hex;
            bench.version += 1;
        }
        if *frame == 45 {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(snapshot.path.clone()));
        }
        if *frame > 70 {
            exit.write(AppExit::Success);
        }
    }
}
