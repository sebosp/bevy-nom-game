//! The files of the player, for the sandbox: the open dialog, a file dropped on the window or
//! on the page.
use bevy::prelude::*;

/// The files opened, waiting for the sandbox: the name and the bytes.
#[derive(Resource, Default)]
pub struct Incoming(pub Vec<(String, Vec<u8>)>);

/// Set to open the dialog on the next frame.
#[derive(Resource, Default)]
pub struct OpenDialog(pub bool);

pub struct FilesPlugin;

impl Plugin for FilesPlugin {
    fn build(&self, app: &mut App) {
        web::listen_for_drops();
        app.init_resource::<Incoming>().init_resource::<OpenDialog>().add_systems(Update, (open_dialog, dropped, web::take_files));
    }
}

/// A file dropped on the window of a desktop.
fn dropped(mut drops: MessageReader<bevy::window::FileDragAndDrop>, mut incoming: ResMut<Incoming>) {
    for drop in drops.read() {
        if let bevy::window::FileDragAndDrop::DroppedFile { path_buf, .. } = drop {
            let name = path_buf.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            match std::fs::read(path_buf) {
                Ok(bytes) => incoming.0.push((name, bytes)),
                Err(err) => warn!("Unable to read {name}: {err}"),
            }
        }
    }
}

/// The open dialog of the desktops, on the main thread (macOS wants it there); in the browser a
/// file input, the file comes back later.
fn open_dialog(mut wanted: ResMut<OpenDialog>, mut incoming: ResMut<Incoming>, _main: bevy::ecs::system::NonSendMarker) {
    if !std::mem::take(&mut wanted.0) {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = rfd::FileDialog::new().pick_file()
        && let Ok(bytes) = std::fs::read(&path)
    {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        incoming.0.push((name, bytes));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = &mut incoming;
        web::pick();
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod web {
    use super::Incoming;
    use bevy::prelude::*;

    pub fn listen_for_drops() {}

    pub fn take_files(_: ResMut<Incoming>) {}
}

/// The files dropped on the page or picked in the browser come in asynchronously: a queue read
/// every frame.
#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;

    use bevy::prelude::*;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    use super::Incoming;

    thread_local! {
        static FILES: RefCell<Vec<(String, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
    }

    fn read_file(file: web_sys::File) {
        let name = file.name();
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(buffer) = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await {
                let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                FILES.with_borrow_mut(|files| files.push((name, bytes)));
            }
        });
    }

    /// The page takes the dragged files instead of opening them.
    pub fn listen_for_drops() {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
        let over = Closure::<dyn Fn(web_sys::DragEvent)>::new(|event: web_sys::DragEvent| event.prevent_default());
        let drop = Closure::<dyn Fn(web_sys::DragEvent)>::new(|event: web_sys::DragEvent| {
            event.prevent_default();
            let Some(files) = event.data_transfer().and_then(|d| d.files()) else { return };
            for i in 0..files.length() {
                if let Some(file) = files.get(i) {
                    read_file(file);
                }
            }
        });
        let _ = document.add_event_listener_with_callback("dragover", over.as_ref().unchecked_ref());
        let _ = document.add_event_listener_with_callback("drop", drop.as_ref().unchecked_ref());
        over.forget();
        drop.forget();
    }

    pub fn pick() {
        wasm_bindgen_futures::spawn_local(async {
            if let Some(file) = rfd::AsyncFileDialog::new().pick_file().await {
                let bytes = file.read().await;
                FILES.with_borrow_mut(|files| files.push((file.file_name(), bytes)));
            }
        });
    }

    pub fn take_files(mut incoming: ResMut<Incoming>) {
        FILES.with_borrow_mut(|files| incoming.0.append(files));
    }
}
