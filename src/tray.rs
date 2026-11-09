//! Tray icon of the service, as a StatusNotifierItem.

use std::sync::Arc;

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, MenuItem, StandardItem, SubMenu};

use crate::daemon::{QUICK_ID, Shared};
use crate::kwin;

pub const ICON_32: &[u8] = include_bytes!("../assets/icon-32.png");
pub const ICON_64: &[u8] = include_bytes!("../assets/icon-64.png");

pub struct Tray {
    shared: Arc<Shared>,
    icons: Vec<ksni::Icon>,
}

/// RGBA pixels of a PNG.
pub fn decode_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).ok()?;
    buffer.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return None,
    };
    Some((info.width, info.height, rgba))
}

fn pixmaps() -> Vec<ksni::Icon> {
    [ICON_32, ICON_64]
        .into_iter()
        .filter_map(decode_png)
        .map(|(width, height, rgba)| ksni::Icon {
            width: width as i32,
            height: height as i32,
            // ARGB32 in network byte order.
            data: rgba
                .chunks(4)
                .flat_map(|p| [p[3], p[0], p[1], p[2]])
                .collect(),
        })
        .collect()
}

fn in_thread(shared: &Arc<Shared>, f: impl FnOnce(Arc<Shared>) + Send + 'static) {
    let shared = shared.clone();
    std::thread::spawn(move || f(shared));
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "tessera".into()
    }

    fn title(&self) -> String {
        "Tessera".into()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::SystemServices
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        let paused = self.shared.engine().paused;
        ksni::ToolTip {
            title: "Tessera".into(),
            description: if paused {
                "Restoring windows is paused".into()
            } else {
                "Window zones and positions".into()
            },
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.shared.show_window();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let (paused, snapshots) = {
            let engine = self.shared.engine();
            let mut list: Vec<(String, String)> = engine
                .snapshots
                .items
                .iter()
                .map(|s| (s.id.clone(), s.name.clone()))
                .collect();
            list.sort_by_key(|(id, _)| id != QUICK_ID);
            (engine.paused, list)
        };

        let restore_items: Vec<MenuItem<Self>> = if snapshots.is_empty() {
            vec![
                StandardItem {
                    label: "Nothing saved yet".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            ]
        } else {
            snapshots
                .into_iter()
                .map(|(id, name)| {
                    StandardItem {
                        label: name.replace('_', "__"),
                        activate: Box::new(move |tray: &mut Self| {
                            let id = id.clone();
                            in_thread(&tray.shared, move |shared| {
                                if let Err(e) = shared.restore_snapshot(&id) {
                                    kwin::osd("dialog-error", &e);
                                }
                            });
                        }),
                        ..Default::default()
                    }
                    .into()
                })
                .collect()
        };

        vec![
            StandardItem {
                label: "Open Tessera".into(),
                icon_name: "view-grid".into(),
                activate: Box::new(|tray: &mut Self| tray.shared.show_window()),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Save arrangement".into(),
                icon_name: "document-save".into(),
                activate: Box::new(|tray: &mut Self| {
                    in_thread(&tray.shared, |shared| {
                        async_io::block_on(shared.shortcut("save"));
                    });
                }),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Restore arrangement".into(),
                icon_name: "document-revert".into(),
                submenu: restore_items,
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Pause restoring windows".into(),
                checked: paused,
                activate: Box::new(|tray: &mut Self| {
                    let paused = !tray.shared.engine().paused;
                    tray.shared.set_paused(paused);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| {
                    tray.shared
                        .quit
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub fn spawn(shared: Arc<Shared>) -> Option<Handle<Tray>> {
    let tray = Tray {
        shared,
        icons: pixmaps(),
    };
    match tray.spawn() {
        Ok(handle) => Some(handle),
        Err(e) => {
            eprintln!("tessera: tray icon unavailable: {e}");
            None
        }
    }
}

/// Rebuild the menu and tooltip from the current state.
pub fn refresh(handle: &Handle<Tray>) {
    handle.update(|_| {});
}

pub fn shutdown(handle: Handle<Tray>) {
    handle.shutdown().wait();
}
