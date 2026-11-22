use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cmd {
    Show,
    ResumeAll,
    PauseAll,
    Quit,
}

pub const CMDS: [Cmd; 4] = [Cmd::Show, Cmd::ResumeAll, Cmd::PauseAll, Cmd::Quit];

pub type OnCmd = Arc<dyn Fn(Cmd) + Send + Sync>;
pub type Status = Box<dyn Fn() -> (String, [String; 4]) + Send>;

pub fn icon(size: u32) -> eframe::egui::IconData {
    let png: &[u8] = match size {
        0..=32 => include_bytes!("../assets/icon-32.png"),
        33..=64 => include_bytes!("../assets/icon-64.png"),
        _ => include_bytes!("../assets/icon-256.png"),
    };
    eframe::icon_data::from_png_bytes(png).unwrap_or_default()
}

#[cfg(target_os = "linux")]
pub fn spawn(on: OnCmd, status: Status) -> bool {
    use ksni::blocking::TrayMethods;

    struct Tray {
        on: OnCmd,
        tip: String,
        labels: [String; 4],
    }

    impl ksni::Tray for Tray {
        fn id(&self) -> String {
            crate::APP_ID.into()
        }
        fn title(&self) -> String {
            crate::APP_NAME.into()
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            [32, 64, 256]
                .into_iter()
                .map(|s| {
                    let i = icon(s);
                    let data = i.rgba.chunks(4).flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
                    ksni::Icon { width: i.width as i32, height: i.height as i32, data }
                })
                .collect()
        }
        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip { title: crate::APP_NAME.into(), description: self.tip.clone(), ..Default::default() }
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            (self.on)(Cmd::Show);
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            let item = |i: usize| -> ksni::MenuItem<Self> {
                ksni::menu::StandardItem { label: self.labels[i].clone(), activate: Box::new(move |t: &mut Self| (t.on)(CMDS[i])), ..Default::default() }.into()
            };
            vec![item(0), ksni::MenuItem::Separator, item(1), item(2), ksni::MenuItem::Separator, item(3)]
        }
    }

    let (tip, labels) = status();
    let Ok(handle) = Tray { on, tip, labels }.spawn() else { return false };
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let (tip, labels) = status();
            if handle.update(|t| (t.tip, t.labels) = (tip, labels)).is_none() {
                break;
            }
        }
    });
    true
}

#[cfg(windows)]
pub fn spawn(on: OnCmd, status: Status) -> bool {
    use std::time::{Duration, Instant};
    use tray_icon::{
        MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage};

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (tip, labels) = status();
        let items: Vec<MenuItem> = labels.iter().map(|l| MenuItem::new(l, true, None)).collect();
        let menu = Menu::new();
        let _ = menu.append(&items[0]);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&items[1]);
        let _ = menu.append(&items[2]);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&items[3]);
        let mut builder = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip(&tip);
        let i = icon(32);
        if let Ok(icon) = tray_icon::Icon::from_rgba(i.rgba, i.width, i.height) {
            builder = builder.with_icon(icon);
        }
        let Ok(tray) = builder.build() else {
            let _ = tx.send(false);
            return;
        };
        let _ = tx.send(true);
        let mut last = Instant::now();
        loop {
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            while let Ok(e) = MenuEvent::receiver().try_recv() {
                if let Some(i) = items.iter().position(|it| it.id() == &e.id) {
                    on(CMDS[i]);
                }
            }
            while let Ok(e) = TrayIconEvent::receiver().try_recv() {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                    on(Cmd::Show);
                }
            }
            if last.elapsed() >= Duration::from_secs(2) {
                let (tip, labels) = status();
                let _ = tray.set_tooltip(Some(&tip));
                items.iter().zip(labels).for_each(|(it, l)| it.set_text(l));
                last = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    rx.recv().unwrap_or(false)
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn spawn(_on: OnCmd, _status: Status) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icons_decode() {
        for (s, expected) in [(22, 32), (32, 32), (48, 64), (64, 64), (256, 256)] {
            let i = icon(s);
            assert_eq!((i.width, i.height), (expected, expected));
            assert_eq!(i.rgba.len(), (expected * expected * 4) as usize);
            assert!(i.rgba.chunks(4).any(|p| p[3] == 0) && i.rgba.chunks(4).any(|p| p[3] == 255));
        }
    }
}
