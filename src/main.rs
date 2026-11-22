#![cfg_attr(windows, windows_subsystem = "windows")]
mod api;
mod i18n;
mod system;
mod tray;

const APP_NAME: &str = "qBit-Distant-UI";
const APP_ID: &str = "qbit-distant-ui";

use api::{AddOpts, Client, Prefs, Source, TFile, Torrent, Tracker};
use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Stroke};
use i18n::Lang;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Server {
    name: String,
    url: String,
    user: String,
    #[serde(skip_serializing)]
    pass: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pass_b64: String,
    #[serde(skip)]
    stored: Option<(String, String)>,
}

impl Server {
    fn key(&self) -> String {
        format!("{}|{}", self.url.trim(), self.user)
    }

    fn load_secret(&mut self) {
        if !self.pass.is_empty() {
            return;
        }
        match system::secret_get(&self.key()) {
            Some(p) => (self.pass, self.stored) = (p.clone(), Some((self.key(), p))),
            None => self.pass = system::b64_decode(&self.pass_b64).unwrap_or_default(),
        }
    }

    fn store_secret(&mut self) {
        let current = (self.key(), self.pass.clone());
        if self.stored.as_ref() == Some(&current) {
            return;
        }
        if system::secret_set(&current.0, &current.1) {
            if let Some((old, _)) = &self.stored
                && *old != current.0
            {
                system::secret_delete(old);
            }
            self.stored = Some(current);
            self.pass_b64.clear();
        } else {
            self.pass_b64 = system::b64_encode(&self.pass);
        }
    }

    fn label(&self) -> &str {
        if self.name.trim().is_empty() { &self.url } else { &self.name }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Watcher {
    enabled: bool,
    folder: String,
    save_path: String,
    category: String,
}

impl Default for Watcher {
    fn default() -> Self {
        Self { enabled: true, folder: String::new(), save_path: String::new(), category: String::new() }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Default)]
enum Sort {
    #[default]
    Added,
    Name,
    Size,
    Progress,
    Speed,
    Ratio,
}

#[derive(Clone, Copy, PartialEq, Default)]
enum Status {
    #[default]
    All,
    Downloading,
    Seeding,
    Done,
    Paused,
    Active,
    Errored,
}

#[derive(Clone, Copy, PartialEq, Default)]
enum Page {
    #[default]
    Torrents,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Default)]
enum DTab {
    #[default]
    Info,
    Files,
    Trackers,
}

#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    #[default]
    General,
    Servers,
    Watch,
    Remote,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    lang: Lang,
    servers: Vec<Server>,
    default_server: Option<usize>,
    selected: usize,
    watchers: Vec<Watcher>,
    ask_on_add: bool,
    notify_done: bool,
    close_to_tray: bool,
    last_save_path: String,
    sort: Sort,
    sort_desc: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: Lang::detect(),
            servers: Vec::new(),
            default_server: None,
            selected: 0,
            watchers: Vec::new(),
            ask_on_add: true,
            notify_done: true,
            close_to_tray: true,
            last_save_path: String::new(),
            sort: Sort::Added,
            sort_desc: true,
        }
    }
}

#[derive(Default)]
struct Shared {
    client: Option<Arc<Client>>,
    torrents: Vec<Torrent>,
    dl: i64,
    up: i64,
    alt_speed: bool,
    categories: Vec<String>,
    default_path: String,
    error: Option<String>,
    poll_error: bool,
    info: Option<(String, Instant)>,
    busy: bool,
    loaded: bool,
    detail: Option<String>,
    files: Option<(String, Vec<TFile>)>,
    files_hold: Option<Instant>,
    dirs: Option<(String, Result<Vec<String>, String>)>,
    prefs: Option<Prefs>,
    incoming: Vec<String>,
    in_flight: HashSet<PathBuf>,
    await_new: Option<HashSet<String>>,
    want_files_tab: bool,
    trackers: Option<(String, Vec<Tracker>)>,
    free: i64,
    incomplete: Option<HashSet<String>>,
    notify: bool,
    lang: Lang,
    ctx: Option<egui::Context>,
    window_open: bool,
    tray_ok: bool,
    quitting: bool,
    close_to_tray: bool,
    watchers: Vec<Watcher>,
}

enum Target {
    Add,
    Watcher(usize),
    Move(String),
}

struct Explorer {
    path: String,
    shown: String,
    target: Target,
}

struct Pending {
    src: Source,
    label: String,
    opts: AddOpts,
}

struct App {
    s: Settings,
    shared: Arc<Mutex<Shared>>,
    ctx: egui::Context,
    filter: String,
    add: String,
    status: Status,
    category: Option<String>,
    to_delete: Option<(String, String)>,
    adds: VecDeque<Pending>,
    explorer: Option<Explorer>,
    page: Page,
    tab: Tab,
    detail_tab: DTab,
    rename: Option<String>,
    limits: Option<(String, i64, i64)>,
    prefs_edit: Option<Prefs>,
    prefs_requested: bool,
    assoc: Option<Result<(), String>>,
    selected: HashSet<String>,
    anchor: Option<String>,
    visible: Vec<String>,
    new_trackers: String,
    logo: egui::TextureHandle,
}

fn finished(prev: &HashSet<String>, now: &[Torrent]) -> Vec<String> {
    now.iter().filter(|t| t.progress >= 1.0 && prev.contains(&t.hash)).map(|t| t.name.clone()).collect()
}

fn poll(sh: Arc<Mutex<Shared>>) {
    let mut tick: u64 = 0;
    loop {
        let (client, detail) = {
            let g = sh.lock().unwrap();
            (g.client.clone(), g.detail.clone())
        };
        if let Some(c) = client {
            let r = c.torrents().and_then(|t| Ok((t, c.transfer()?)));
            let alt = c.alt_speed().ok();
            let cats = c.categories().ok();
            let free = (tick % 15 == 0).then(|| c.free_space().ok()).flatten();
            let files = detail.clone().and_then(|h| c.files(&h).ok().map(|f| (h, f)));
            let trackers = detail.and_then(|h| c.trackers(&h).ok().map(|f| (h, f)));
            let mut done = Vec::new();
            let mut g = sh.lock().unwrap();
            if g.client.as_ref().is_some_and(|x| Arc::ptr_eq(x, &c)) {
                match r {
                    Ok((t, tr)) => {
                        let incomplete = t.iter().filter(|t| t.progress < 1.0).map(|t| t.hash.clone()).collect();
                        if let Some(prev) = &g.incomplete {
                            done = finished(prev, &t);
                        }
                        g.incomplete = Some(incomplete);
                        (g.torrents, g.dl, g.up, g.loaded) = (t, tr.dl_info_speed, tr.up_info_speed, true);
                        if g.poll_error {
                            (g.error, g.poll_error) = (None, false);
                        }
                    }
                    Err(e) => (g.error, g.poll_error) = (Some(e), true),
                }
                if let Some(a) = alt {
                    g.alt_speed = a;
                }
                if let Some(c) = cats {
                    g.categories = c;
                }
                if let Some(f) = free {
                    g.free = f;
                }
                let held = g.files_hold.is_some_and(|t| t.elapsed() < Duration::from_secs(3));
                if !held && files.as_ref().is_some_and(|(h, _)| g.detail.as_ref() == Some(h)) {
                    g.files = files;
                }
                if trackers.as_ref().is_some_and(|(h, _)| g.detail.as_ref() == Some(h)) {
                    g.trackers = trackers;
                }
                let new = g.await_new.as_ref().and_then(|b| g.torrents.iter().find(|t| !b.contains(&t.hash)).map(|t| t.hash.clone()));
                if let Some(h) = new {
                    (g.detail, g.files, g.trackers, g.await_new, g.want_files_tab) = (Some(h), None, None, None, true);
                }
            }
            let (notify, title) = (g.notify, g.lang.t("notif_done"));
            if !done.is_empty() {
                g.info = Some((format!("{title} : {}", done.join(", ")), Instant::now()));
            }
            drop(g);
            if notify {
                for name in &done {
                    system::notify(title, name);
                }
            }
            repaint(&sh);
            scan_watchers(&sh, &c);
            tick += 1;
        }
        thread::sleep(Duration::from_secs(2));
    }
}

fn repaint(sh: &Arc<Mutex<Shared>>) {
    let ctx = sh.lock().unwrap().ctx.clone();
    if let Some(c) = ctx {
        c.request_repaint();
    }
}

fn scan_watchers(sh: &Arc<Mutex<Shared>>, c: &Arc<Client>) {
    let (watchers, sent) = {
        let g = sh.lock().unwrap();
        (g.watchers.clone(), g.lang.t("watch_sent"))
    };
    for w in watchers.iter().filter(|w| w.enabled && !w.folder.trim().is_empty()) {
        let Ok(rd) = std::fs::read_dir(w.folder.trim()) else { continue };
        for p in rd.flatten().map(|e| e.path()) {
            if !p.is_file() || !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("torrent")) {
                continue;
            }
            if !sh.lock().unwrap().in_flight.insert(p.clone()) {
                continue;
            }
            let (c, sh) = (c.clone(), sh.clone());
            let opts = AddOpts { save_path: w.save_path.clone(), category: w.category.clone(), start: true };
            thread::spawn(move || {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let r = std::fs::read(&p).map_err(|e| e.to_string()).and_then(|d| c.add(&Source::File(name.clone(), d), &opts));
                let mut g = sh.lock().unwrap();
                match r {
                    Ok(()) => {
                        match std::fs::remove_file(&p) {
                            Ok(()) => drop(g.in_flight.remove(&p)),
                            Err(e) => g.error = Some(format!("{} : {e}", p.display())),
                        }
                        g.info = Some((format!("{sent} {name}"), Instant::now()));
                    }
                    Err(e) => {
                        if e != "err_rejected" {
                            g.in_flight.remove(&p);
                        }
                        g.error = Some(format!("{name} : {e}"));
                    }
                }
                drop(g);
                repaint(&sh);
            });
        }
    }
}

fn focus(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}

fn open_window(sh: &Arc<Mutex<Shared>>, wake: &mpsc::Sender<tray::Cmd>) {
    let ctx = {
        let g = sh.lock().unwrap();
        g.ctx.clone().filter(|_| g.window_open)
    };
    match ctx {
        Some(c) => focus(&c),
        None => drop(wake.send(tray::Cmd::Show)),
    }
}

fn handle_cmd(sh: &Arc<Mutex<Shared>>, wake: &mpsc::Sender<tray::Cmd>, cmd: tray::Cmd) {
    match cmd {
        tray::Cmd::Show => open_window(sh, wake),
        tray::Cmd::ResumeAll | tray::Cmd::PauseAll => {
            if let Some(c) = sh.lock().unwrap().client.clone() {
                let run = cmd == tray::Cmd::ResumeAll;
                thread::spawn(move || drop(c.set_running("all", run)));
            }
        }
        tray::Cmd::Quit => {
            let ctx = {
                let mut g = sh.lock().unwrap();
                g.quitting = true;
                g.ctx.clone().filter(|_| g.window_open)
            };
            match ctx {
                Some(c) => c.send_viewport_cmd(egui::ViewportCommand::Close),
                None => drop(wake.send(tray::Cmd::Quit)),
            }
        }
    }
}

fn tray_status(sh: &Arc<Mutex<Shared>>) -> (String, [String; 4]) {
    let g = sh.lock().unwrap();
    let l = g.lang;
    let tip = if g.client.is_some() { format!("⬇ {}/s   ⬆ {}/s", bytes(l, g.dl), bytes(l, g.up)) } else { l.t("connect").to_string() };
    (tip, [l.t("tray_show"), l.t("start_all"), l.t("stop_all"), l.t("quit")].map(String::from))
}

fn start_background(sh: &Arc<Mutex<Shared>>, wake: mpsc::Sender<tray::Cmd>, desktop: bool) {
    let s = sh.clone();
    thread::spawn(move || poll(s));
    if !desktop {
        return;
    }
    let (s, w) = (sh.clone(), wake.clone());
    system::listen(move |items| {
        s.lock().unwrap().incoming.extend(items);
        open_window(&s, &w);
    });
    let (s, s2) = (sh.clone(), sh.clone());
    let on: tray::OnCmd = Arc::new(move |cmd| handle_cmd(&s, &wake, cmd));
    let ok = tray::spawn(on, Box::new(move || tray_status(&s2)));
    sh.lock().unwrap().tray_ok = ok;
}

fn apply_style(ctx: &egui::Context) {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |s| {
            let dark = theme == egui::Theme::Dark;
            let a = Color32::from_rgba_unmultiplied;
            let v = &mut s.visuals;
            v.window_fill = if dark { a(28, 26, 46, 246) } else { a(246, 246, 253, 248) };
            v.panel_fill = Color32::TRANSPARENT;
            v.window_stroke = Stroke::new(1.0, if dark { a(255, 255, 255, 36) } else { a(255, 255, 255, 235) });
            v.window_corner_radius = CornerRadius::same(16);
            v.menu_corner_radius = CornerRadius::same(12);
            v.window_shadow = egui::Shadow { offset: [0, 10], blur: 32, spread: 0, color: a(0, 0, 0, if dark { 110 } else { 40 }) };
            v.popup_shadow = egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: a(0, 0, 0, if dark { 90 } else { 30 }) };
            v.extreme_bg_color = if dark { a(0, 0, 0, 70) } else { a(255, 255, 255, 200) };
            v.faint_bg_color = if dark { a(255, 255, 255, 8) } else { a(255, 255, 255, 90) };
            v.selection.bg_fill = if dark { a(110, 96, 235, 200) } else { a(150, 136, 255, 150) };
            let w = &mut v.widgets;
            for (st, alpha) in [(&mut w.inactive, 18u8), (&mut w.hovered, 34), (&mut w.active, 50), (&mut w.open, 30)] {
                let fill = if dark { a(255, 255, 255, alpha) } else { a(255, 255, 255, alpha.saturating_mul(5)) };
                (st.bg_fill, st.weak_bg_fill, st.corner_radius) = (fill, fill, CornerRadius::same(8));
                st.bg_stroke = Stroke::new(1.0, if dark { a(255, 255, 255, alpha / 2 + 6) } else { a(190, 190, 225, 130) });
            }
            w.noninteractive.corner_radius = CornerRadius::same(8);
            w.noninteractive.bg_stroke = Stroke::new(1.0, if dark { a(255, 255, 255, 22) } else { a(0, 0, 0, 22) });
        });
    }
}

impl App {
    fn new(cc: &eframe::CreationContext, shared: Arc<Mutex<Shared>>) -> Self {
        let mut s: Settings = cc.storage.and_then(|st| eframe::get_value(st, "settings")).unwrap_or_else(|| {
            let mut s = Settings::default();
            if let Some(old) = cc.storage.and_then(|st| eframe::get_value::<Server>(st, eframe::APP_KEY)).filter(|o| !o.url.is_empty()) {
                s.servers.push(old);
                s.default_server = Some(0);
            }
            s
        });
        apply_style(&cc.egui_ctx);
        let already = {
            let mut g = shared.lock().unwrap();
            (g.ctx, g.window_open) = (Some(cc.egui_ctx.clone()), true);
            g.client.is_some()
        };
        s.servers.iter_mut().for_each(Server::load_secret);
        let first_run = s.servers.is_empty();
        if first_run {
            s.servers.push(Server { url: "http://localhost:8080".into(), user: "admin".into(), ..Default::default() });
        }
        let mut app = Self {
            s,
            shared,
            ctx: cc.egui_ctx.clone(),
            filter: String::new(),
            add: String::new(),
            status: Status::All,
            category: None,
            to_delete: None,
            adds: VecDeque::new(),
            explorer: None,
            page: if first_run { Page::Settings } else { Page::Torrents },
            tab: if first_run { Tab::Servers } else { Tab::General },
            detail_tab: DTab::Info,
            rename: None,
            limits: None,
            prefs_edit: None,
            prefs_requested: false,
            assoc: None,
            selected: HashSet::new(),
            anchor: None,
            visible: Vec::new(),
            new_trackers: String::new(),
            logo: {
                let i = tray::icon(64);
                cc.egui_ctx.load_texture("logo", egui::ColorImage::from_rgba_unmultiplied([i.width as usize, i.height as usize], &i.rgba), egui::TextureOptions::LINEAR)
            },
        };
        if !already && let Some(i) = app.s.default_server {
            app.connect(i);
        }
        app
    }

    fn client(&self) -> Option<Arc<Client>> {
        self.shared.lock().unwrap().client.clone()
    }

    fn connect(&mut self, i: usize) {
        let Some(sv) = self.s.servers.get(i).cloned() else { return };
        self.s.selected = i;
        (self.prefs_edit, self.prefs_requested) = (None, false);
        let c = Arc::new(Client::new(&sv.url, &sv.user, &sv.pass));
        let (sh, ctx) = (self.shared.clone(), self.ctx.clone());
        {
            let mut g = sh.lock().unwrap();
            (g.busy, g.client, g.loaded, g.detail, g.files, g.incomplete, g.free) = (true, None, false, None, None, None, -1);
            g.torrents.clear();
        }
        thread::spawn(move || {
            let r = c.login().map(|_| c.default_save_path().unwrap_or_default());
            let mut g = sh.lock().unwrap();
            g.busy = false;
            match r {
                Ok(p) => (g.client, g.default_path, g.error) = (Some(c), p, None),
                Err(e) => g.error = Some(e),
            }
            ctx.request_repaint();
        });
    }

    fn disconnect(&mut self) {
        let mut g = self.shared.lock().unwrap();
        (g.client, g.loaded, g.dl, g.up, g.detail, g.files) = (None, false, 0, 0, None, None);
        g.torrents.clear();
        drop(g);
        (self.prefs_edit, self.prefs_requested) = (None, false);
    }

    fn fetch<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Client) -> Result<T, String> + Send + 'static,
        store: impl FnOnce(&mut Shared, T) + Send + 'static,
    ) {
        let Some(c) = self.client() else { return };
        let (sh, ctx) = (self.shared.clone(), self.ctx.clone());
        thread::spawn(move || {
            let r = f(&c);
            let mut g = sh.lock().unwrap();
            match r {
                Ok(v) => store(&mut g, v),
                Err(e) => (g.error, g.poll_error) = (Some(e), false),
            }
            ctx.request_repaint();
        });
    }

    fn run(&self, f: impl FnOnce(&Client) -> Result<(), String> + Send + 'static) {
        self.fetch(f, |_, ()| {});
    }


    fn queue_item(&mut self, item: String) {
        if system::is_url(&item) {
            return self.queue(Source::Urls(item));
        }
        match std::fs::read(&item) {
            Ok(d) => {
                let name = Path::new(&item).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.queue(Source::File(name, d));
            }
            Err(e) => self.shared.lock().unwrap().error = Some(format!("{item} : {e}")),
        }
    }

    fn queue(&mut self, src: Source) {
        let save_path = if self.s.last_save_path.is_empty() { self.shared.lock().unwrap().default_path.clone() } else { self.s.last_save_path.clone() };
        let opts = AddOpts { save_path, category: String::new(), start: true };
        if self.s.ask_on_add {
            self.adds.push_back(Pending { label: source_label(&src), src, opts });
        } else {
            self.submit(src, opts);
        }
    }

    fn submit(&mut self, src: Source, opts: AddOpts) {
        if !opts.save_path.trim().is_empty() {
            self.s.last_save_path = opts.save_path.trim().to_string();
        }
        if !opts.start {
            let mut g = self.shared.lock().unwrap();
            let known = g.torrents.iter().map(|t| t.hash.clone()).collect();
            g.await_new = Some(known);
        }
        let msg = self.s.lang.t("added_ok").to_string();
        self.fetch(move |c| c.add(&src, &opts), move |g, ()| g.info = Some((msg, Instant::now())));
    }

    fn open_explorer(&mut self, path: String, target: Target) {
        let mut path = path.trim().to_string();
        if path.is_empty() {
            path = self.shared.lock().unwrap().default_path.clone();
        }
        if path.is_empty() {
            path = "/".into();
        }
        self.explorer = Some(Explorer { path: path.clone(), shown: path.clone(), target });
        self.load_dirs(path);
    }

    fn load_dirs(&self, path: String) {
        self.shared.lock().unwrap().dirs = None;
        let p = path.clone();
        self.fetch(move |c| Ok(c.dirs(&p)), move |g, r| g.dirs = Some((path, r)));
    }

    fn open_details(&mut self, hash: &str) {
        let mut g = self.shared.lock().unwrap();
        (g.detail, g.files, g.trackers) = (Some(hash.to_string()), None, None);
        drop(g);
        (self.detail_tab, self.rename, self.limits) = (DTab::Info, None, None);
    }

    fn ask_delete_selection(&mut self) {
        let hashes = self.selected.iter().cloned().collect::<Vec<_>>().join("|");
        self.to_delete = Some((hashes, format!("{} {}", self.selected.len(), self.s.lang.t("n_torrents"))));
    }

    fn selection_bar(&mut self, ui: &mut egui::Ui, list: &[Torrent]) {
        let l = self.s.lang;
        let hashes = self.selected.iter().cloned().collect::<Vec<_>>().join("|");
        let first_path = list.iter().find(|t| self.selected.contains(&t.hash)).map(|t| t.save_path.clone()).unwrap_or_default();
        let cats = self.shared.lock().unwrap().categories.clone();
        let mut move_req = false;
        glass(ui).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} {}", self.selected.len(), l.t("selected"))).strong());
                ui.separator();
                for (label, run) in [(format!("▶ {}", l.t("resume")), true), (format!("⏸ {}", l.t("pause")), false)] {
                    if ui.button(label).clicked() {
                        let h = hashes.clone();
                        self.run(move |c| c.set_running(&h, run));
                    }
                }
                if ui.button(l.t("recheck")).clicked() {
                    let h = hashes.clone();
                    self.run(move |c| c.recheck(&h));
                }
                if ui.button(l.t("reannounce")).clicked() {
                    let h = hashes.clone();
                    self.run(move |c| c.reannounce(&h));
                }
                egui::ComboBox::from_id_salt("bulkcat")
                    .selected_text(l.t("set_category"))
                    .show_ui(ui, |ui| {
                        for c in std::iter::once(String::new()).chain(cats.iter().cloned()) {
                            let label = if c.is_empty() { l.t("none").to_string() } else { c.clone() };
                            if ui.selectable_label(false, label).clicked() {
                                let h = hashes.clone();
                                self.run(move |cl| cl.set_category(&h, &c));
                            }
                        }
                    })
                    .response
                    .accessible_name(l.t("set_category"));
                move_req = ui.button(l.t("move")).clicked();
                if ui.button(format!("🗑 {}", l.t("delete"))).clicked() {
                    self.ask_delete_selection();
                }
                ui.separator();
                if ui.button(l.t("select_visible")).clicked() {
                    self.selected.extend(self.visible.iter().cloned());
                }
                if ui.button(l.t("clear_selection")).clicked() {
                    self.selected.clear();
                }
            });
        });
        if move_req {
            self.open_explorer(first_path, Target::Move(hashes));
        }
        ui.add_space(8.0);
    }

    fn pick_torrents(&self) {
        if let Some(files) = rfd::FileDialog::new().add_filter("Torrent", &["torrent"]).pick_files() {
            self.shared.lock().unwrap().incoming.extend(files.into_iter().map(|p| p.to_string_lossy().into_owned()));
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, connected: bool) {
        if !connected || self.page != Page::Torrents {
            return;
        }
        let typing = ctx.memory(|m| m.focused().is_some());
        let (open, find, pasted, all, del, esc) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::O),
                i.modifiers.command && i.key_pressed(egui::Key::F),
                i.events.iter().find_map(|e| if let egui::Event::Paste(s) = e { Some(s.clone()) } else { None }),
                i.modifiers.command && i.key_pressed(egui::Key::A),
                i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::Escape),
            )
        });
        let dialogs = self.to_delete.is_some() || !self.adds.is_empty() || self.explorer.is_some();
        if !typing && all {
            self.selected.extend(self.visible.iter().cloned());
        }
        if !typing && del && !dialogs && !self.selected.is_empty() {
            self.ask_delete_selection();
        }
        if !typing && esc && !dialogs {
            self.selected.clear();
        }
        if open {
            self.pick_torrents();
        }
        if find {
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique("filter")));
        }
        if !typing && let Some(p) = pasted {
            for line in p.lines().map(str::trim).filter(|l| system::is_url(l)) {
                self.queue(Source::Urls(line.to_string()));
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, connected: bool, busy: bool) {
        let l = self.s.lang;
        let (dl, up, alt, error, info) = {
            let g = self.shared.lock().unwrap();
            (g.dl, g.up, g.alt_speed, g.error.clone(), g.info.clone())
        };
        glass(ui).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Image::from_texture(egui::load::SizedTexture::from_handle(&self.logo)).fit_to_exact_size(egui::vec2(34.0, 34.0)).alt_text(APP_NAME));
                ui.separator();
                if connected {
                    let (cdl, cup) = speed_colors(ui);
                    ui.label(RichText::new(format!("⬇ {}/s", bytes(l, dl))).color(cdl));
                    ui.label(RichText::new(format!("⬆ {}/s", bytes(l, up))).color(cup));
                    if ui.selectable_label(alt, "🐢").on_hover_text(l.t("alt_speed")).clicked() {
                        self.run(|c| c.toggle_alt_speed());
                        self.shared.lock().unwrap().alt_speed = !alt;
                    }
                    ui.label(RichText::new(self.s.servers.get(self.s.selected).map(Server::label).unwrap_or_default()).weak());
                    let free = self.shared.lock().unwrap().free;
                    if free >= 0 {
                        ui.label(RichText::new(format!("🖴 {} {}", bytes(l, free), l.t("free_space"))).weak());
                    }
                    if let Some((msg, at)) = &info {
                        let left = Duration::from_secs(5).saturating_sub(at.elapsed());
                        if !left.is_zero() {
                            ui.separator();
                            ui.add(egui::Label::new(RichText::new(format!("✔ {msg}")).color(speed_colors(ui).1)).truncate());
                            ui.ctx().request_repaint_after(left);
                        }
                    }
                } else if self.s.servers.is_empty() {
                    ui.label(RichText::new(l.t("no_server")).weak());
                } else {
                    self.s.selected = self.s.selected.min(self.s.servers.len() - 1);
                    egui::ComboBox::from_id_salt("server")
                        .selected_text(self.s.servers[self.s.selected].label().to_string())
                        .show_ui(ui, |ui| {
                            for (i, sv) in self.s.servers.iter().enumerate() {
                                ui.selectable_value(&mut self.s.selected, i, sv.label());
                            }
                        })
                        .response
                        .accessible_name(l.t("tab_servers"));
                    if ui.add_enabled(!busy, egui::Button::new(l.t("connect"))).clicked() {
                        self.connect(self.s.selected);
                    }
                    if busy {
                        ui.spinner();
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙").on_hover_text(l.t("settings")).clicked() {
                        self.page = if self.page == Page::Settings { Page::Torrents } else { Page::Settings };
                    }
                    egui::widgets::global_theme_preference_buttons(ui);
                    if connected && ui.button(l.t("disconnect")).clicked() {
                        self.disconnect();
                    }
                });
            });
            if let Some(e) = error {
                ui.horizontal(|ui| {
                    ui.colored_label(Color32::from_rgb(235, 90, 90), format!("⚠ {}", l.msg(&e)));
                    if ui.small_button("✖").clicked() {
                        self.shared.lock().unwrap().error = None;
                    }
                });
            }
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let l = self.s.lang;
        let cats = self.shared.lock().unwrap().categories.clone();
        glass(ui).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button(format!("🗁 {}", l.t("open_file"))).on_hover_text(l.t("shortcuts")).clicked() {
                    self.pick_torrents();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(l.t("stop_all")).clicked() {
                        self.run(|c| c.set_running("all", false));
                    }
                    if ui.button(l.t("start_all")).clicked() {
                        self.run(|c| c.set_running("all", true));
                    }
                    ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text(format!("🔍 {}", l.t("filter"))).desired_width(150.0).id(egui::Id::unique("filter"))).accessible_name(l.t("filter"));
                    let add = ui.button(format!("➕ {}", l.t("add")));
                    let r = ui.add(egui::TextEdit::singleline(&mut self.add).hint_text(l.t("add_hint")).desired_width(ui.available_width())).accessible_name(l.t("add_hint"));
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (add.clicked() || enter) && !self.add.trim().is_empty() {
                        let urls = std::mem::take(&mut self.add).trim().to_string();
                        self.queue(Source::Urls(urls));
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for (st, k) in [
                    (Status::All, "f_all"),
                    (Status::Downloading, "f_dl"),
                    (Status::Seeding, "f_seed"),
                    (Status::Done, "f_done"),
                    (Status::Paused, "f_paused"),
                    (Status::Active, "f_active"),
                    (Status::Errored, "f_error"),
                ] {
                    ui.selectable_value(&mut self.status, st, l.t(k));
                }
                ui.separator();
                egui::ComboBox::from_id_salt("cat")
                    .selected_text(self.category.clone().unwrap_or_else(|| l.t("cat_all").into()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.category, None, l.t("cat_all"));
                        for c in &cats {
                            ui.selectable_value(&mut self.category, Some(c.clone()), c);
                        }
                    })
                    .response
                    .accessible_name(l.t("category"));
                let sorts = [
                    (Sort::Added, "sort_added"),
                    (Sort::Name, "sort_name"),
                    (Sort::Size, "sort_size"),
                    (Sort::Progress, "sort_progress"),
                    (Sort::Speed, "sort_speed"),
                    (Sort::Ratio, "sort_ratio"),
                ];
                let current = sorts.iter().find(|(s, _)| *s == self.s.sort).map(|(_, k)| l.t(k)).unwrap_or_default();
                egui::ComboBox::from_id_salt("sort").selected_text(current).show_ui(ui, |ui| {
                    for (s, k) in sorts {
                        ui.selectable_value(&mut self.s.sort, s, l.t(k));
                    }
                })
                .response
                .accessible_name(l.t("sort"));
                if ui.button(if self.s.sort_desc { "⬇" } else { "⬆" }).on_hover_text(l.t("sort_dir")).clicked() {
                    self.s.sort_desc = !self.s.sort_desc;
                }
            });
        });
    }

    fn row_menu(&mut self, ui: &mut egui::Ui, t: &Torrent) {
        let l = self.s.lang;
        if ui.button(l.t("details")).clicked() {
            self.open_details(&t.hash);
            ui.close();
        }
        if ui.button(l.t("move")).clicked() {
            self.open_explorer(t.save_path.clone(), Target::Move(t.hash.clone()));
            ui.close();
        }
        if ui.button(l.t("recheck")).clicked() {
            let h = t.hash.clone();
            self.run(move |c| c.recheck(&h));
            ui.close();
        }
        if ui.button(l.t("reannounce")).clicked() {
            let h = t.hash.clone();
            self.run(move |c| c.reannounce(&h));
            ui.close();
        }
        if !t.magnet_uri.is_empty() && ui.button(l.t("copy_magnet")).clicked() {
            ui.ctx().copy_text(t.magnet_uri.clone());
            ui.close();
        }
    }

    fn list(&mut self, ui: &mut egui::Ui) {
        let l = self.s.lang;
        let f = self.filter.to_lowercase();
        let (loaded, mut list): (bool, Vec<Torrent>) = {
            let g = self.shared.lock().unwrap();
            if g.loaded {
                self.selected.retain(|h| g.torrents.iter().any(|t| &t.hash == h));
            }
            let list = g
                .torrents
                .iter()
                .filter(|t| (f.is_empty() || t.name.to_lowercase().contains(&f)) && matches(self.status, t) && self.category.as_ref().is_none_or(|c| &t.category == c))
                .cloned()
                .collect();
            (g.loaded, list)
        };
        if !loaded {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() / 3.0);
                ui.spinner();
                ui.label(RichText::new(l.t("loading")).weak());
            });
            return;
        }
        if list.is_empty() {
            ui.centered_and_justified(|ui| ui.label(RichText::new(l.t("no_torrent")).weak()));
            return;
        }
        sort_list(&mut list, self.s.sort, self.s.sort_desc);
        self.visible = list.iter().map(|t| t.hash.clone()).collect();
        if !self.selected.is_empty() {
            self.selection_bar(ui, &list);
        }
        let row_h = 76.0;
        let mut clicked_sel: Option<(String, bool)> = None;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, list.len(), |ui, range| {
            for t in &list[range] {
                let (key, paused) = state_label(&t.state);
                let sel = self.selected.contains(&t.hash);
                let mut frame = glass(ui).inner_margin(Margin::symmetric(12, 8));
                if sel {
                    frame = frame.stroke(Stroke::new(1.5, ui.visuals().selection.bg_fill));
                }
                frame.show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("🗑").on_hover_text(l.t("delete")).clicked() {
                                self.to_delete = Some((t.hash.clone(), t.name.clone()));
                            }
                            let (icon, tip) = if paused { ("▶", l.t("resume")) } else { ("⏸", l.t("pause")) };
                            if ui.button(icon).on_hover_text(tip).clicked() {
                                let h = t.hash.clone();
                                self.run(move |c| c.set_running(&h, paused));
                            }
                            ui.menu_button("☰", |ui| self.row_menu(ui, t));
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                let mut on = sel;
                                if ui.checkbox(&mut on, "").accessible_name(&t.name).changed() {
                                    clicked_sel = Some((t.hash.clone(), on));
                                }
                                let r = ui.add(egui::Label::new(RichText::new(&t.name).strong()).truncate().sense(egui::Sense::click()));
                                if r.clicked() {
                                    if ui.input(|i| i.modifiers.command || i.modifiers.shift) {
                                        clicked_sel = Some((t.hash.clone(), !sel));
                                    } else {
                                        self.open_details(&t.hash);
                                    }
                                }
                                r.context_menu(|ui| self.row_menu(ui, t));
                            });
                        });
                    });
                    ui.add(egui::ProgressBar::new(t.progress as f32).desired_height(6.0).corner_radius(3));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{:.1} %", t.progress * 100.0)).small());
                        ui.label(RichText::new(l.t(key)).small().weak());
                        ui.label(
                            RichText::new(format!(
                                "{} · ⬇ {}/s · ⬆ {}/s · ratio {:.2} · {}",
                                bytes(l, t.size),
                                bytes(l, t.dlspeed),
                                bytes(l, t.upspeed),
                                t.ratio,
                                eta(l, t.eta)
                            ))
                            .small()
                            .weak(),
                        );
                        if !t.category.is_empty() {
                            ui.label(RichText::new(format!("🏷 {}", t.category)).small());
                        }
                    });
                });
                ui.add_space(row_h - 72.0);
            }
        });
        if let Some((h, on)) = clicked_sel {
            let shift = ui.input(|i| i.modifiers.shift);
            let pos = |x: &str| list.iter().position(|t| t.hash == x);
            match (shift, self.anchor.as_deref().and_then(pos), pos(&h)) {
                (true, Some(a), Some(b)) => self.selected.extend(list[a.min(b)..=a.max(b)].iter().map(|t| t.hash.clone())),
                _ if on => drop(self.selected.insert(h.clone())),
                _ => drop(self.selected.remove(&h)),
            }
            self.anchor = Some(h);
        }
    }

    fn delete_modal(&mut self, ctx: &egui::Context) {
        let l = self.s.lang;
        let Some((hash, name)) = self.to_delete.clone() else { return };
        let modal = egui::Modal::new(egui::Id::unique("delete")).backdrop_color(Color32::from_black_alpha(90)).show(ctx, |ui| {
            ui.set_max_width(380.0);
            ui.label(RichText::new(l.t("delete_q")).strong());
            ui.add(egui::Label::new(&name).truncate());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (txt, files) in [(l.t("delete"), false), (l.t("delete_files"), true)] {
                    if ui.button(txt).clicked() {
                        let h = hash.clone();
                        self.run(move |c| c.delete(&h, files));
                        self.selected.retain(|x| !hash.split('|').any(|d| d == x));
                        self.to_delete = None;
                    }
                }
                if ui.button(l.t("cancel")).clicked() {
                    self.to_delete = None;
                }
            });
        });
        if modal.should_close() {
            self.to_delete = None;
        }
    }

    fn add_modal(&mut self, ctx: &egui::Context, connected: bool) {
        let l = self.s.lang;
        let cats = self.shared.lock().unwrap().categories.clone();
        let more = self.adds.len().saturating_sub(1);
        let Some(p) = self.adds.front_mut() else { return };
        let (mut browse, mut submit, mut cancel) = (false, false, false);
        let m = egui::Modal::new(egui::Id::unique("add")).backdrop_color(Color32::from_black_alpha(90)).show(ctx, |ui| {
            ui.set_width(500.0);
            ui.heading(l.t("add_title"));
            ui.add(egui::Label::new(RichText::new(&p.label).strong()).truncate());
            if more > 0 {
                ui.label(RichText::new(format!("+{more} {}", l.t("queued_more"))).weak());
            }
            ui.add_space(6.0);
            egui::Grid::new("addgrid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label(l.t("save_to"));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut p.opts.save_path).desired_width(300.0)).accessible_name(l.t("save_to"));
                    browse = ui.add_enabled(connected, egui::Button::new(format!("🗀 {}", l.t("browse")))).clicked();
                });
                ui.end_row();
                ui.label(l.t("category"));
                category_combo(ui, "addcat", &mut p.opts.category, &cats, l);
                ui.end_row();
            });
            ui.checkbox(&mut p.opts.start, l.t("start_now"));
            ui.label(RichText::new(l.t("pick_after")).small().weak());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                submit = ui.add_enabled(connected, egui::Button::new(l.t("add"))).clicked();
                cancel = ui.button(l.t("cancel")).clicked();
            });
        });
        let path = p.opts.save_path.clone();
        if m.should_close() && self.explorer.is_none() {
            cancel = true;
        }
        if browse {
            self.open_explorer(path, Target::Add);
        } else if submit {
            let p = self.adds.pop_front().unwrap();
            self.submit(p.src, p.opts);
        } else if cancel {
            self.adds.pop_front();
        }
    }

    fn explorer_modal(&mut self, ctx: &egui::Context) {
        let l = self.s.lang;
        let dirs = self.shared.lock().unwrap().dirs.clone();
        let Some(ex) = &mut self.explorer else { return };
        let (mut go, mut choose, mut cancel) = (None::<String>, false, false);
        let m = egui::Modal::new(egui::Id::unique("explorer")).backdrop_color(Color32::from_black_alpha(90)).show(ctx, |ui| {
            ui.set_width(500.0);
            ui.heading(l.t("explorer_title"));
            ui.horizontal(|ui| {
                if ui.button("⬆").on_hover_text(l.t("parent")).clicked() {
                    go = Some(parent(&ex.path));
                }
                let r = ui.add(egui::TextEdit::singleline(&mut ex.path).desired_width(ui.available_width())).accessible_name(l.t("location"));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    go = Some(ex.path.clone());
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| match &dirs {
                Some((p, Ok(list))) if *p == ex.shown => {
                    if list.is_empty() {
                        ui.label(RichText::new(l.t("empty_dir")).weak());
                    }
                    for d in list {
                        if ui.selectable_label(false, format!("🗀 {}", leaf(d))).clicked() {
                            go = Some(d.clone());
                        }
                    }
                }
                Some((p, Err(e))) if *p == ex.shown => {
                    let msg = if e == "err_notfound" { l.t("explorer_unavailable").to_string() } else { l.msg(e) };
                    ui.colored_label(Color32::from_rgb(235, 90, 90), msg);
                }
                _ => {
                    ui.spinner();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                choose = ui.button(l.t("choose")).clicked();
                cancel = ui.button(l.t("cancel")).clicked();
            });
        });
        if m.should_close() {
            cancel = true;
        }
        if let Some(p) = go {
            if let Some(ex) = &mut self.explorer {
                (ex.path, ex.shown) = (p.clone(), p.clone());
            }
            self.load_dirs(p);
        } else if choose {
            let ex = self.explorer.take().unwrap();
            let path = ex.path.trim().to_string();
            match ex.target {
                Target::Add => {
                    if let Some(p) = self.adds.front_mut() {
                        p.opts.save_path = path;
                    }
                }
                Target::Watcher(i) => {
                    if let Some(w) = self.s.watchers.get_mut(i) {
                        w.save_path = path;
                    }
                }
                Target::Move(h) => self.run(move |c| c.set_location(&h, &path)),
            }
        } else if cancel {
            self.explorer = None;
        }
    }

    fn details_window(&mut self, ctx: &egui::Context) {
        let l = self.s.lang;
        let (hash, t, files, trackers, cats, files_tab) = {
            let mut g = self.shared.lock().unwrap();
            let Some(h) = g.detail.clone() else { return };
            let t = g.torrents.iter().find(|t| t.hash == h).cloned();
            let files = g.files.as_ref().filter(|(fh, _)| *fh == h).map(|(_, f)| f.clone());
            let trackers = g.trackers.as_ref().filter(|(th, _)| *th == h).map(|(_, f)| f.clone());
            let files_tab = std::mem::take(&mut g.want_files_tab);
            (h, t, files, trackers, g.categories.clone(), files_tab)
        };
        if files_tab {
            self.detail_tab = DTab::Files;
        }
        if let Some(t) = &t
            && self.limits.as_ref().is_none_or(|(h, ..)| *h != hash)
        {
            self.limits = Some((hash.clone(), t.dl_limit.max(0) / 1024, t.up_limit.max(0) / 1024));
        }
        type Act = Box<dyn FnOnce(&Client) -> Result<(), String> + Send>;
        type Patch = Box<dyn FnOnce(&mut Torrent)>;
        let mut acts: Vec<Act> = Vec::new();
        let mut patches: Vec<Patch> = Vec::new();
        let (mut open, mut prio, mut mv) = (true, None::<(Vec<i64>, i64)>, false);
        egui::Window::new(l.t("details")).open(&mut open).default_size([700.0, 520.0]).collapsible(false).show(ctx, |ui| {
            let Some(t) = &t else {
                ui.spinner();
                return;
            };
            ui.horizontal(|ui| {
                if let Some(name) = &mut self.rename {
                    let r = ui.add(egui::TextEdit::singleline(name).desired_width((ui.available_width() - 170.0).max(120.0))).accessible_name(l.t("rename"));
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button(l.t("save")).clicked() || enter {
                        let (h, n) = (hash.clone(), name.trim().to_string());
                        if !n.is_empty() {
                            let n2 = n.clone();
                            acts.push(Box::new(move |c| c.rename(&h, &n)));
                            patches.push(Box::new(move |t| t.name = n2));
                        }
                        self.rename = None;
                    } else if ui.button(l.t("cancel")).clicked() {
                        self.rename = None;
                    }
                } else {
                    if ui.small_button("✏").on_hover_text(l.t("rename")).clicked() {
                        self.rename = Some(t.name.clone());
                    }
                    ui.add(egui::Label::new(RichText::new(&t.name).strong().size(15.0)).truncate());
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for (tab, k) in [(DTab::Info, "info"), (DTab::Files, "files"), (DTab::Trackers, "trackers")] {
                    ui.selectable_value(&mut self.detail_tab, tab, l.t(k));
                }
            });
            ui.separator();
            match self.detail_tab {
                DTab::Info => {
                    let (key, paused) = state_label(&t.state);
                    egui::Grid::new("detailgrid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        ui.label(l.t("location"));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&t.save_path).monospace());
                            mv = ui.small_button(l.t("move")).clicked();
                        });
                        ui.end_row();
                        ui.label(l.t("category"));
                        let mut cat = t.category.clone();
                        category_combo(ui, "dcat", &mut cat, &cats, l);
                        if cat != t.category {
                            let (h, c2) = (hash.clone(), cat.clone());
                            acts.push(Box::new(move |c| c.set_category(&h, &c2)));
                            patches.push(Box::new(move |t| t.category = cat));
                        }
                        ui.end_row();
                        ui.label(l.t("size"));
                        ui.label(format!("{} · {:.1} % · ratio {:.2}", bytes(l, t.size), t.progress * 100.0, t.ratio));
                        ui.end_row();
                        ui.label(l.t("seeds_peers"));
                        ui.label(format!("{} / {}", t.num_seeds, t.num_leechs));
                        ui.end_row();
                        ui.label(l.t("state"));
                        ui.horizontal(|ui| {
                            ui.label(l.t(key));
                            let (icon, tip) = if paused { ("▶", l.t("resume")) } else { ("⏸", l.t("pause")) };
                            if ui.small_button(format!("{icon} {tip}")).clicked() {
                                let h = hash.clone();
                                acts.push(Box::new(move |c| c.set_running(&h, paused)));
                            }
                        });
                        ui.end_row();
                    });
                    ui.separator();
                    let mut force = t.force_start;
                    if ui.checkbox(&mut force, l.t("force_start")).changed() {
                        let h = hash.clone();
                        acts.push(Box::new(move |c| c.set_force_start(&h, force)));
                        patches.push(Box::new(move |t| t.force_start = force));
                    }
                    let mut seq = t.seq_dl;
                    if ui.checkbox(&mut seq, l.t("sequential")).changed() {
                        let h = hash.clone();
                        acts.push(Box::new(move |c| c.toggle_sequential(&h)));
                        patches.push(Box::new(move |t| t.seq_dl = seq));
                    }
                    let mut fl = t.f_l_piece_prio;
                    if ui.checkbox(&mut fl, l.t("first_last")).changed() {
                        let h = hash.clone();
                        acts.push(Box::new(move |c| c.toggle_first_last(&h)));
                        patches.push(Box::new(move |t| t.f_l_piece_prio = fl));
                    }
                    ui.add_space(6.0);
                    ui.label(l.t("limits"));
                    if let Some((_, dl, up)) = &mut self.limits {
                        ui.horizontal(|ui| {
                            ui.label(l.t("dl_short"));
                            ui.add(egui::DragValue::new(dl).range(0..=10_000_000).suffix(" KiB/s")).accessible_name(l.t("dl_short"));
                            ui.label(l.t("up_short"));
                            ui.add(egui::DragValue::new(up).range(0..=10_000_000).suffix(" KiB/s")).accessible_name(l.t("up_short"));
                            if ui.button(l.t("apply")).clicked() {
                                let (h, d, u) = (hash.clone(), *dl * 1024, *up * 1024);
                                acts.push(Box::new(move |c| c.set_limits(&h, d, u)));
                                patches.push(Box::new(move |t| (t.dl_limit, t.up_limit) = (d, u)));
                            }
                        });
                    }
                }
                DTab::Files => {
                    if let Some(f) = files.as_ref().filter(|f| !f.is_empty()) {
                        ui.horizontal(|ui| {
                            let ids: Vec<i64> = f.iter().map(|f| f.index).collect();
                            if ui.small_button(l.t("select_all")).clicked() {
                                prio = Some((ids.clone(), 1));
                            }
                            if ui.small_button(l.t("select_none")).clicked() {
                                prio = Some((ids, 0));
                            }
                        });
                        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                            for file in f {
                                ui.horizontal(|ui| {
                                    let mut on = file.priority > 0;
                                    if ui.checkbox(&mut on, "").accessible_name(format!("{} : {}", l.t("files"), file.name)).changed() {
                                        prio = Some((vec![file.index], on as i64));
                                    }
                                    ui.label(RichText::new(format!("{:>5.1} % · {}", file.progress * 100.0, bytes(l, file.size))).small().weak());
                                    ui.add(egui::Label::new(&file.name).truncate());
                                });
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new(l.t("waiting_meta")).weak());
                        });
                    }
                }
                DTab::Trackers => {
                    let joined = self.new_trackers.lines().map(str::trim).filter(|u| !u.is_empty()).collect::<Vec<_>>().join("\n");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.new_trackers)
                                .hint_text(l.t("trackers_hint"))
                                .desired_rows(2)
                                .desired_width((ui.available_width() - 200.0).max(160.0)),
                        )
                        .accessible_name(l.t("add_trackers"));
                        if ui.add_enabled(!joined.is_empty(), egui::Button::new(format!("➕ {}", l.t("add_trackers")))).clicked() {
                            let h = hash.clone();
                            acts.push(Box::new(move |c| c.add_trackers(&h, &joined)));
                            self.new_trackers.clear();
                        }
                    });
                    ui.separator();
                    match &trackers {
                        Some(list) if !list.is_empty() => {
                            egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                                for tr in list {
                                    let key = match tr.status {
                                        0 => "tr_0",
                                        1 => "tr_1",
                                        2 => "tr_2",
                                        3 => "tr_3",
                                        _ => "tr_4",
                                    };
                                    let color = match tr.status {
                                        2 => speed_colors(ui).1,
                                        4 => Color32::from_rgb(235, 90, 90),
                                        _ => ui.visuals().weak_text_color(),
                                    };
                                    let n = |v: i64| if v < 0 { "–".to_string() } else { v.to_string() };
                                    ui.horizontal(|ui| {
                                        if ui.small_button("✖").on_hover_text(l.t("remove_tracker")).clicked() {
                                            let (h, u) = (hash.clone(), tr.url.clone());
                                            acts.push(Box::new(move |c| c.remove_trackers(&h, &u)));
                                        }
                                        ui.colored_label(color, RichText::new(l.t(key)).small());
                                        ui.label(RichText::new(format!("{} / {}", n(tr.num_seeds), n(tr.num_leeches))).small().weak());
                                        ui.add(egui::Label::new(RichText::new(&tr.url).monospace()).truncate());
                                    });
                                    if !tr.msg.is_empty() {
                                        ui.label(RichText::new(&tr.msg).small().weak());
                                    }
                                }
                            });
                        }
                        Some(_) => {
                            ui.label(RichText::new(l.t("no_tracker")).weak());
                        }
                        None => {
                            ui.spinner();
                        }
                    }
                }
            }
        });
        if !open {
            let mut g = self.shared.lock().unwrap();
            (g.detail, g.files, g.trackers) = (None, None, None);
            drop(g);
            (self.rename, self.limits) = (None, None);
            return;
        }
        if let Some((ids, p)) = prio {
            {
                let mut g = self.shared.lock().unwrap();
                if let Some((_, f)) = g.files.as_mut() {
                    f.iter_mut().filter(|f| ids.contains(&f.index)).for_each(|f| f.priority = p);
                }
                g.files_hold = Some(Instant::now());
            }
            let h = hash.clone();
            self.run(move |c| c.set_file_prio(&h, &ids, p));
        }
        if !patches.is_empty() {
            let mut g = self.shared.lock().unwrap();
            if let Some(t) = g.torrents.iter_mut().find(|t| t.hash == hash) {
                patches.into_iter().for_each(|p| p(t));
            }
        }
        for a in acts {
            self.run(a);
        }
        if mv && let Some(t) = t {
            self.open_explorer(t.save_path, Target::Move(hash));
        }
    }

    fn settings_page(&mut self, ui: &mut egui::Ui, connected: bool) {
        let l = self.s.lang;
        glass(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                if ui.button(format!("⬅ {}", l.t("back"))).clicked() {
                    self.page = Page::Torrents;
                }
                ui.separator();
                ui.label(RichText::new(l.t("settings")).strong().size(18.0));
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for (tab, k) in [(Tab::General, "tab_general"), (Tab::Servers, "tab_servers"), (Tab::Watch, "tab_watch"), (Tab::Remote, "tab_remote")] {
                    ui.selectable_value(&mut self.tab, tab, l.t(k));
                }
            });
        });
        ui.add_space(10.0);
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            glass(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                match self.tab {
                    Tab::General => self.tab_general(ui),
                    Tab::Servers => self.tab_servers(ui),
                    Tab::Watch => self.tab_watch(ui, connected),
                    Tab::Remote => self.tab_remote(ui, connected),
                }
            });
        });
    }

    fn tab_general(&mut self, ui: &mut egui::Ui) {
        let l = self.s.lang;
        egui::Grid::new("general").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
            ui.label(l.t("language"));
            egui::ComboBox::from_id_salt("lang").selected_text(self.s.lang.name()).show_ui(ui, |ui| {
                for x in Lang::ALL {
                    ui.selectable_value(&mut self.s.lang, x, x.name());
                }
            })
            .response
            .accessible_name(l.t("language"));
            ui.end_row();
            ui.label(l.t("theme"));
            egui::widgets::global_theme_preference_buttons(ui);
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.checkbox(&mut self.s.ask_on_add, l.t("ask_on_add"));
        ui.checkbox(&mut self.s.notify_done, format!("🔔 {}", l.t("notify_done")));
        ui.checkbox(&mut self.s.close_to_tray, l.t("close_to_tray"));
        if !self.shared.lock().unwrap().tray_ok {
            ui.label(RichText::new(l.t("tray_unavailable")).small().weak());
        }
        ui.label(RichText::new(l.t("shortcuts")).small().weak());
        ui.add_space(10.0);
        if ui.button(format!("🔗 {}", l.t("associate"))).clicked() {
            self.assoc = Some(system::associate());
        }
        match &self.assoc {
            Some(Ok(())) => {
                ui.colored_label(speed_colors(ui).1, l.t("associate_ok"));
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::from_rgb(235, 90, 90), l.msg(e));
            }
            None => {}
        }
    }

    fn tab_servers(&mut self, ui: &mut egui::Ui) {
        let l = self.s.lang;
        let (mut remove, mut connect) = (None, None);
        for (i, sv) in self.s.servers.iter_mut().enumerate() {
            glass(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                egui::Grid::new(("srv", i)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label(l.t("name"));
                    ui.add(egui::TextEdit::singleline(&mut sv.name).desired_width(320.0)).accessible_name(l.t("name"));
                    ui.end_row();
                    ui.label(l.t("url"));
                    ui.add(egui::TextEdit::singleline(&mut sv.url).hint_text("https://…").desired_width(320.0)).accessible_name(l.t("url"));
                    ui.end_row();
                    ui.label(l.t("user"));
                    ui.add(egui::TextEdit::singleline(&mut sv.user).desired_width(320.0)).accessible_name(l.t("user"));
                    ui.end_row();
                    ui.label(l.t("password"));
                    ui.add(egui::TextEdit::singleline(&mut sv.pass).password(true).desired_width(320.0)).accessible_name(l.t("password"));
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    let is_default = self.s.default_server == Some(i);
                    if ui.radio(is_default, l.t("default")).on_hover_text(l.t("default_hint")).clicked() {
                        self.s.default_server = if is_default { None } else { Some(i) };
                    }
                    if ui.button(l.t("connect")).clicked() {
                        connect = Some(i);
                    }
                    if ui.button("🗑").on_hover_text(l.t("delete")).clicked() {
                        remove = Some(i);
                    }
                });
            });
            ui.add_space(6.0);
        }
        if ui.button(format!("➕ {}", l.t("add_server"))).clicked() {
            self.s.servers.push(Server { url: "https://".into(), ..Default::default() });
        }
        if let Some(i) = remove {
            let gone = self.s.servers.remove(i);
            if let Some((key, _)) = gone.stored {
                system::secret_delete(&key);
            }
            self.s.default_server = match self.s.default_server {
                Some(d) if d == i => None,
                Some(d) if d > i => Some(d - 1),
                d => d,
            };
            if self.s.selected >= i && self.s.selected > 0 {
                self.s.selected -= 1;
            }
        }
        if let Some(i) = connect {
            self.connect(i);
        }
    }

    fn tab_watch(&mut self, ui: &mut egui::Ui, connected: bool) {
        let l = self.s.lang;
        let cats = self.shared.lock().unwrap().categories.clone();
        ui.label(RichText::new(l.t("watch_help")).weak());
        ui.add_space(6.0);
        let (mut remove, mut browse) = (None, None);
        for (i, w) in self.s.watchers.iter_mut().enumerate() {
            glass(ui).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                egui::Grid::new(("watch", i)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label(l.t("local_folder"));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut w.folder).desired_width(320.0)).accessible_name(l.t("local_folder"));
                        if ui.button("🗁").on_hover_text(l.t("browse")).clicked()
                            && let Some(d) = rfd::FileDialog::new().pick_folder()
                        {
                            w.folder = d.to_string_lossy().into_owned();
                        }
                    });
                    ui.end_row();
                    ui.label(l.t("remote_folder"));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut w.save_path).desired_width(320.0)).accessible_name(l.t("remote_folder"));
                        if ui.add_enabled(connected, egui::Button::new("🗀")).on_hover_text(l.t("browse")).clicked() {
                            browse = Some(i);
                        }
                    });
                    ui.end_row();
                    ui.label(l.t("category"));
                    category_combo(ui, ("wcat", i), &mut w.category, &cats, l);
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut w.enabled, l.t("enabled"));
                    if ui.button("🗑").on_hover_text(l.t("delete")).clicked() {
                        remove = Some(i);
                    }
                });
            });
            ui.add_space(6.0);
        }
        if ui.button(format!("➕ {}", l.t("add_watch"))).clicked() {
            self.s.watchers.push(Watcher::default());
        }
        if let Some(i) = remove {
            self.s.watchers.remove(i);
        }
        if let Some(i) = browse {
            let p = self.s.watchers[i].save_path.clone();
            self.open_explorer(p, Target::Watcher(i));
        }
    }

    fn tab_remote(&mut self, ui: &mut egui::Ui, connected: bool) {
        let l = self.s.lang;
        if !connected {
            ui.label(RichText::new(l.t("connect_first")).weak());
            return;
        }
        if self.prefs_edit.is_none() {
            if let Some(p) = self.shared.lock().unwrap().prefs.take() {
                self.prefs_edit = Some(p);
            } else if !self.prefs_requested {
                self.prefs_requested = true;
                self.fetch(|c| c.prefs(), |g, p| g.prefs = Some(p));
            }
        }
        let Some(p) = &mut self.prefs_edit else {
            ui.spinner();
            return;
        };
        let mut apply = false;
        egui::Grid::new("remote").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
            for (key, v) in [("dl_limit", &mut p.dl_limit), ("up_limit", &mut p.up_limit)] {
                ui.label(l.t(key));
                let mut kib = (*v).max(0) / 1024;
                if ui.add(egui::DragValue::new(&mut kib).range(0..=10_000_000).suffix(" KiB/s")).accessible_name(l.t(key)).changed() {
                    *v = kib * 1024;
                }
                ui.end_row();
            }
            ui.checkbox(&mut p.max_ratio_enabled, l.t("ratio_limit"));
            ui.add_enabled(p.max_ratio_enabled, egui::DragValue::new(&mut p.max_ratio).speed(0.05).range(0.0..=1000.0).fixed_decimals(2)).accessible_name(l.t("ratio_limit"));
            ui.end_row();
            ui.checkbox(&mut p.max_seeding_time_enabled, l.t("seed_limit"));
            ui.add_enabled(p.max_seeding_time_enabled, egui::DragValue::new(&mut p.max_seeding_time).range(0..=525_600)).accessible_name(l.t("seed_limit"));
            ui.end_row();
            ui.label(l.t("then_action"));
            let acts = [(0, "act_stop"), (1, "act_remove"), (3, "act_remove_files"), (2, "act_superseed")];
            let current = acts.iter().find(|(v, _)| *v == p.max_ratio_act).map(|(_, k)| l.t(k)).unwrap_or_default();
            egui::ComboBox::from_id_salt("act").selected_text(current).show_ui(ui, |ui| {
                for (v, k) in acts {
                    ui.selectable_value(&mut p.max_ratio_act, v, l.t(k));
                }
            })
            .response
            .accessible_name(l.t("then_action"));
            ui.end_row();
        });
        ui.label(RichText::new(l.t("auto_remove_help")).small().weak());
        ui.add_space(8.0);
        apply |= ui.button(l.t("apply")).clicked();
        if apply {
            if p.max_ratio_enabled && p.max_ratio < 0.0 {
                p.max_ratio = 1.0;
            }
            if p.max_seeding_time_enabled && p.max_seeding_time < 0 {
                p.max_seeding_time = 1440;
            }
            let send = p.clone();
            let msg = l.t("saved").to_string();
            self.fetch(move |c| c.set_prefs(&send), move |g, ()| g.info = Some((msg, Instant::now())));
        }
    }
}

fn category_combo(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, value: &mut String, cats: &[String], l: Lang) {
    let text = if value.is_empty() { l.t("none").to_string() } else { value.clone() };
    egui::ComboBox::from_id_salt(id).selected_text(text).show_ui(ui, |ui| {
        ui.selectable_value(value, String::new(), l.t("none"));
        for c in cats {
            ui.selectable_value(value, c.clone(), c);
        }
    })
    .response
    .accessible_name(l.t("category"));
}

fn source_label(src: &Source) -> String {
    match src {
        Source::File(n, _) => n.clone(),
        Source::Urls(u) => u
            .split(['?', '&'])
            .find_map(|p| p.strip_prefix("dn="))
            .map(|d| system::percent_decode(&d.replace('+', " ")))
            .unwrap_or_else(|| u.chars().take(90).collect()),
    }
}

fn parent(p: &str) -> String {
    let t = p.trim_end_matches(['/', '\\']);
    if t.is_empty() {
        return p.to_string();
    }
    match t.rfind(['/', '\\']) {
        Some(i) => {
            let head = &t[..i];
            if head.is_empty() || head.ends_with(':') { t[..=i].to_string() } else { head.to_string() }
        }
        None => t.to_string(),
    }
}

fn leaf(p: &str) -> &str {
    p.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(p)
}

fn matches(s: Status, t: &Torrent) -> bool {
    let st = t.state.as_str();
    match s {
        Status::All => true,
        Status::Downloading => matches!(st, "downloading" | "forcedDL" | "stalledDL" | "metaDL" | "forcedMetaDL" | "queuedDL" | "checkingDL"),
        Status::Seeding => matches!(st, "uploading" | "forcedUP" | "stalledUP" | "queuedUP" | "checkingUP"),
        Status::Done => t.progress >= 1.0,
        Status::Paused => state_label(st).1,
        Status::Active => t.dlspeed > 0 || t.upspeed > 0,
        Status::Errored => matches!(st, "error" | "missingFiles"),
    }
}

fn sort_list(list: &mut [Torrent], sort: Sort, desc: bool) {
    list.sort_by(|a, b| {
        let o = match sort {
            Sort::Added => a.added_on.cmp(&b.added_on),
            Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Sort::Size => a.size.cmp(&b.size),
            Sort::Progress => a.progress.total_cmp(&b.progress),
            Sort::Speed => (a.dlspeed + a.upspeed).cmp(&(b.dlspeed + b.upspeed)),
            Sort::Ratio => a.ratio.total_cmp(&b.ratio),
        };
        if desc { o.reverse() } else { o }
    });
}

fn speed_colors(ui: &egui::Ui) -> (Color32, Color32) {
    if ui.visuals().dark_mode {
        (Color32::from_rgb(80, 180, 255), Color32::from_rgb(120, 210, 140))
    } else {
        (Color32::from_rgb(20, 110, 200), Color32::from_rgb(30, 140, 70))
    }
}

fn glass(ui: &egui::Ui) -> Frame {
    let (fill, stroke) = if ui.visuals().dark_mode {
        (Color32::from_rgba_unmultiplied(255, 255, 255, 14), Color32::from_rgba_unmultiplied(255, 255, 255, 30))
    } else {
        (Color32::from_rgba_unmultiplied(255, 255, 255, 120), Color32::from_rgba_unmultiplied(255, 255, 255, 200))
    };
    Frame::new().fill(fill).stroke(Stroke::new(1.0, stroke)).corner_radius(CornerRadius::same(14)).inner_margin(Margin::same(12))
}

fn paint_background(ui: &egui::Ui) {
    let r = ui.max_rect();
    let p = ui.painter();
    let (top, bottom, blob_a, blob_b) = if ui.visuals().dark_mode {
        ((18, 20, 34), (30, 18, 44), (90, 70, 200, 70), (20, 140, 170, 55))
    } else {
        ((226, 232, 250), (245, 228, 240), (150, 130, 255, 80), (90, 200, 220, 70))
    };
    let mut mesh = egui::Mesh::default();
    let c = |(r, g, b)| Color32::from_rgb(r, g, b);
    for (pos, col) in [(r.left_top(), top), (r.right_top(), top), (r.right_bottom(), bottom), (r.left_bottom(), bottom)] {
        mesh.colored_vertex(pos, c(col));
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(mesh);
    let blob = |center: egui::Pos2, radius: f32, (r, g, b, a): (u8, u8, u8, u8)| {
        let mut m = egui::Mesh::default();
        m.colored_vertex(center, Color32::from_rgba_unmultiplied(r, g, b, a));
        let n = 64;
        for i in 0..=n {
            let t = i as f32 / n as f32 * std::f32::consts::TAU;
            m.colored_vertex(center + radius * egui::vec2(t.cos(), t.sin()), Color32::TRANSPARENT);
        }
        for i in 1..=n {
            m.add_triangle(0, i, i + 1);
        }
        p.add(m);
    };
    blob(r.left_top() + egui::vec2(r.width() * 0.2, r.height() * 0.3), r.width() * 0.3, blob_a);
    blob(r.right_bottom() - egui::vec2(r.width() * 0.15, r.height() * 0.2), r.width() * 0.35, blob_b);
}

fn bytes(l: Lang, n: i64) -> String {
    let units = if l == Lang::Fr { ["o", "Ko", "Mo", "Go", "To"] } else { ["B", "KB", "MB", "GB", "TB"] };
    let mut v = n.max(0) as f64;
    for (i, u) in units.iter().enumerate() {
        if v < 1024.0 || i == units.len() - 1 {
            return if i == 0 { format!("{v:.0} {u}") } else { format!("{v:.1} {u}") };
        }
        v /= 1024.0;
    }
    unreachable!()
}

fn eta(l: Lang, s: i64) -> String {
    let d = match l {
        Lang::Fr => "j",
        Lang::En => "d",
        Lang::Ja => "日",
    };
    match s {
        s if s <= 0 || s >= 8_640_000 => "∞".into(),
        s if s < 3600 => format!("{}m{:02}s", s / 60, s % 60),
        s if s < 86400 => format!("{}h{:02}m", s / 3600, s % 3600 / 60),
        s => format!("{}{d}{}h", s / 86400, s % 86400 / 3600),
    }
}

fn state_label(s: &str) -> (&'static str, bool) {
    let paused = s.starts_with("paused") || s.starts_with("stopped");
    let key = match s {
        "downloading" | "forcedDL" => "st_dl",
        "uploading" | "forcedUP" | "stalledUP" => "st_seed",
        "stalledDL" => "st_stalled",
        "metaDL" | "forcedMetaDL" => "st_meta",
        "queuedDL" | "queuedUP" => "st_queued",
        "checkingDL" | "checkingUP" | "checkingResumeData" => "st_check",
        "moving" => "st_moving",
        "error" | "missingFiles" => "st_error",
        _ if paused && s.ends_with("UP") => "st_done",
        _ if paused => "st_paused",
        _ => "st_unknown",
    };
    (key, paused)
}

impl Drop for App {
    fn drop(&mut self) {
        if let Ok(mut g) = self.shared.lock() {
            (g.ctx, g.window_open) = (None, false);
        }
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.s.servers.iter_mut().for_each(Server::store_secret);
        eframe::set_value(storage, "settings", &self.s);
    }

    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.client().is_none() {
            return;
        }
        let items = std::mem::take(&mut self.shared.lock().unwrap().incoming);
        for item in items {
            self.queue_item(item);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        paint_background(ui);
        let dropped: Vec<String> = ui.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_string_lossy().into_owned()).collect());
        let (connected, busy) = {
            let mut g = self.shared.lock().unwrap();
            g.incoming.extend(dropped);
            (g.notify, g.lang, g.close_to_tray) = (self.s.notify_done, self.s.lang, self.s.close_to_tray);
            g.watchers.clone_from(&self.s.watchers);
            (g.client.is_some(), g.busy)
        };
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx, connected);
        Frame::new().inner_margin(Margin::same(14)).show(ui, |ui| {
            self.top_bar(ui, connected, busy);
            ui.add_space(10.0);
            match self.page {
                Page::Settings => self.settings_page(ui, connected),
                Page::Torrents if connected => {
                    self.toolbar(ui);
                    ui.add_space(10.0);
                    self.list(ui);
                }
                Page::Torrents => {}
            }
        });

        self.delete_modal(&ctx);
        self.details_window(&ctx);
        self.add_modal(&ctx, connected);
        self.explorer_modal(&ctx);
    }
}

fn main() -> eframe::Result {
    let items = system::cli_items();
    if system::forward(&items) {
        return Ok(());
    }
    if let (Some(old), Some(new)) = (eframe::storage_dir("qbit-ui"), eframe::storage_dir(APP_ID))
        && old.join("app.ron").exists()
        && !new.join("app.ron").exists()
        && std::fs::create_dir_all(&new).is_ok()
    {
        let _ = std::fs::copy(old.join("app.ron"), new.join("app.ron"));
    }
    let shared = Arc::new(Mutex::new(Shared { incoming: items, free: -1, ..Default::default() }));
    let (wake, woken) = mpsc::channel();
    start_background(&shared, wake, true);
    loop {
        let sh = shared.clone();
        let icon = tray::icon(256);
        let opts = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1040.0, 680.0])
                .with_min_inner_size([620.0, 380.0])
                .with_title(APP_NAME)
                .with_app_id(APP_ID)
                .with_icon(icon),
            ..Default::default()
        };
        eframe::run_native(APP_ID, opts, Box::new(move |cc| Ok(Box::new(App::new(cc, sh)))))?;
        while woken.try_recv().is_ok() {}
        let keep = {
            let g = shared.lock().unwrap();
            g.tray_ok && g.close_to_tray && !g.quitting
        };
        if !keep {
            return Ok(());
        }
        loop {
            match woken.recv() {
                Ok(tray::Cmd::Show) => break,
                Ok(tray::Cmd::Quit) | Err(_) => return Ok(()),
                Ok(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatters() {
        assert_eq!(bytes(Lang::Fr, 512), "512 o");
        assert_eq!(bytes(Lang::En, 1536), "1.5 KB");
        assert_eq!(eta(Lang::Fr, 8_640_000), "∞");
        assert_eq!(eta(Lang::Ja, 90_000), "1日1h");
        assert_eq!(state_label("stoppedUP"), ("st_done", true));
        assert_eq!(state_label("pausedDL"), ("st_paused", true));
        assert_eq!(state_label("downloading"), ("st_dl", false));
    }

    #[test]
    fn password_never_serialized_in_clear() {
        let sv = Server { name: "n".into(), url: "http://x".into(), user: "u".into(), pass: "hunter2".into(), pass_b64: system::b64_encode("hunter2"), stored: None };
        let ron = ron_like(&sv);
        assert!(!ron.contains("hunter2"));
        let legacy: Server = serde_json::from_str(r#"{"name":"n","url":"http://x","user":"u","pass":"old"}"#).unwrap();
        assert_eq!(legacy.pass, "old");
    }

    fn ron_like(sv: &Server) -> String {
        serde_json::to_string(sv).unwrap()
    }

    #[test]
    fn completion_detection() {
        let mk = |h: &str, p: f64| Torrent { hash: h.into(), name: h.into(), progress: p, ..Default::default() };
        let prev: HashSet<String> = ["a", "b"].into_iter().map(String::from).collect();
        assert_eq!(finished(&prev, &[mk("a", 1.0), mk("b", 0.5), mk("c", 1.0)]), vec!["a".to_string()]);
    }

    #[test]
    fn paths_and_labels() {
        assert_eq!(parent("/downloads/anime/"), "/downloads");
        assert_eq!(parent("/downloads"), "/");
        assert_eq!(parent("/"), "/");
        assert_eq!(parent("C:\\Users\\me"), "C:\\Users");
        assert_eq!(parent("C:\\Users"), "C:\\");
        assert_eq!(leaf("/downloads/anime/"), "anime");
        assert_eq!(source_label(&Source::Urls("magnet:?xt=urn:btih:x&dn=Big+Buck%20Bunny".into())), "Big Buck Bunny");
        let mk = |n: &str, s: i64| Torrent { name: n.into(), size: s, ..Default::default() };
        let mut v = vec![mk("b", 1), mk("a", 3), mk("c", 2)];
        sort_list(&mut v, Sort::Size, true);
        assert_eq!(v.iter().map(|t| t.name.as_str()).collect::<String>(), "acb");
        sort_list(&mut v, Sort::Name, false);
        assert_eq!(v.iter().map(|t| t.name.as_str()).collect::<String>(), "abc");
    }
}

#[cfg(test)]
mod ui_live {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    const SINTEL: &str = "08ada5a7a6183aae1e09d831df6748d566095a10";

    #[derive(Debug)]
    struct Dropped(std::path::PathBuf);
    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|e| e.to_string())
        }
    }

    pub(super) fn wait(h: &mut Harness<App>, what: &str, ok: impl Fn(&App, &Shared) -> bool) {
        for _ in 0..250 {
            h.step();
            let done = ok(h.state(), &h.state().shared.lock().unwrap());
            if done {
                h.run_steps(3);
                return;
            }
            thread::sleep(Duration::from_millis(100));
        }
        panic!("délai dépassé : {what} (erreur : {:?})", h.state().shared.lock().unwrap().error);
    }

    fn state_of(s: &Shared, hash: &str) -> Option<String> {
        s.torrents.iter().find(|t| t.hash == hash).map(|t| t.state.clone())
    }

    fn shot(h: &mut Harness<App>, name: &str) {
        if let Ok(dir) = std::env::var("QBIT_SHOTS") {
            h.render().unwrap().save(format!("{dir}/{name}.png")).unwrap();
        }
    }

    pub(super) fn cjk(ctx: &egui::Context) {
        let Ok(o) = std::process::Command::new("fc-match").args(["-f", "%{file}", "Noto Sans CJK JP"]).output() else { return };
        let Ok(data) = std::fs::read(String::from_utf8_lossy(&o.stdout).as_ref()) else { return };
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("cjk".into(), Arc::new(egui::FontData::from_owned(data)));
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().push("cjk".into());
        ctx.set_fonts(fonts);
    }

    #[test]
    #[ignore]
    fn ui_live() {
        let shared = Arc::new(Mutex::new(Shared { free: -1, ..Default::default() }));
        let (wake, _woken) = mpsc::channel();
        start_background(&shared, wake, false);
        let sh = shared.clone();
        let mut h = Harness::builder().with_size([1040.0, 680.0]).wgpu().build_eframe(move |cc| App::new(cc, sh));
        cjk(&h.ctx);
        {
            let a = h.state_mut();
            a.s.lang = Lang::Fr;
            a.s.servers = vec![Server {
                name: "Local".into(),
                url: std::env::var("QBIT_URL").unwrap_or("http://localhost:8080".into()),
                user: std::env::var("QBIT_USER").unwrap_or_default(),
                pass: std::env::var("QBIT_PASS").unwrap_or_default(),
                ..Default::default()
            }];
            a.page = Page::Torrents;
        }
        h.ctx.set_theme(egui::ThemePreference::Dark);
        h.run_steps(2);
        shot(&mut h, "01-connexion");

        h.get_by_label("Connexion").click();
        wait(&mut h, "connexion + liste", |_, s| s.client.is_some() && s.loaded && !s.torrents.is_empty());
        shot(&mut h, "02-liste");

        h.state_mut().add = format!("magnet:?xt=urn:btih:{SINTEL}&dn=Sintel");
        h.get_by_label("➕ Ajouter").click();
        wait(&mut h, "fenêtre d'ajout", |a, _| !a.adds.is_empty());
        shot(&mut h, "03-ajout");
        h.get_by_label("🗀 Parcourir").click();
        wait(&mut h, "explorateur", |a, s| a.explorer.is_some() && s.dirs.as_ref().is_some_and(|(_, r)| r.is_ok()));
        shot(&mut h, "04-explorateur");
        h.get_by_label("⬆").click();
        wait(&mut h, "dossier parent", |a, s| a.explorer.as_ref().is_some_and(|e| e.shown == "/") && s.dirs.as_ref().is_some_and(|(p, r)| p == "/" && r.is_ok()));
        h.get_by_label("🗀 downloads").click_accesskit();
        wait(&mut h, "retour /downloads", |a, s| a.explorer.as_ref().is_some_and(|e| e.shown == "/downloads") && s.dirs.as_ref().is_some_and(|(p, _)| p == "/downloads"));
        h.get_by_label("Choisir ce dossier").click();
        h.run_steps(3);
        assert_eq!(h.state().adds.front().unwrap().opts.save_path, "/downloads");
        h.get_by_label("Ajouter").click();
        wait(&mut h, "ajout magnet", |_, s| state_of(s, SINTEL).is_some());
        h.state_mut().filter = "sintel".into();
        h.run_steps(3);

        h.get_by_label("⏸").click();
        wait(&mut h, "pause", |_, s| state_of(s, SINTEL).is_some_and(|st| state_label(&st).1));
        h.get_by_label("▶").click();
        wait(&mut h, "reprise", |_, s| state_of(s, SINTEL).is_some_and(|st| !state_label(&st).1));
        h.get_by_label("☰").click();
        h.run_steps(3);
        shot(&mut h, "05-menu");
        h.get_by_label("Détails").click();
        wait(&mut h, "détails", |_, s| s.detail.as_deref() == Some(SINTEL));
        shot(&mut h, "06-details-magnet");
        h.state_mut().shared.lock().unwrap().detail = None;
        h.run_steps(2);
        h.get_by_role(egui::accesskit::Role::CheckBox).click();
        h.run_steps(3);
        assert!(h.state().selected.contains(SINTEL));
        h.get_by_label("⏸ Pause").click();
        wait(&mut h, "pause groupée", |_, s| state_of(s, SINTEL).is_some_and(|st| state_label(&st).1));
        shot(&mut h, "06b-selection");
        h.get_by_label("🗑 Supprimer").click();
        h.run_steps(3);
        h.get_by_label("Supprimer + fichiers").click();
        wait(&mut h, "suppression groupée", |a, s| state_of(s, SINTEL).is_none() && a.selected.is_empty());

        if let Ok(path) = std::env::var("QBIT_TORRENT") {
            let hash = std::env::var("QBIT_TORRENT_HASH").unwrap();
            h.state_mut().filter = "qbit-ui-test".into();
            h.input_mut().dropped_files.push(Arc::new(Dropped(path.into())));
            wait(&mut h, "fenêtre d'ajout .torrent", |a, _| !a.adds.is_empty());
            h.state_mut().adds.front_mut().unwrap().opts.start = false;
            h.get_by_label("Ajouter").click();
            wait(&mut h, "détails auto + fichiers", |_, s| s.detail.as_deref() == Some(hash.as_str()) && s.files.as_ref().is_some_and(|(_, f)| !f.is_empty()));
            shot(&mut h, "07-details-fichiers");
            assert!(h.state().detail_tab == DTab::Files);
            h.get_by_label("Fichiers : qbit-ui-test.txt").click();
            h.run_steps(2);
            thread::sleep(Duration::from_secs(4));
            wait(&mut h, "priorité 0 côté serveur", |_, s| s.files.as_ref().is_some_and(|(_, f)| f[0].priority == 0));
            h.state_mut().detail_tab = DTab::Info;
            h.run_steps(3);
            h.get_by_label("Téléchargement séquentiel (pratique pour regarder pendant le téléchargement)").click();
            wait(&mut h, "séquentiel côté serveur", |_, s| s.torrents.iter().any(|t| t.hash == hash && t.seq_dl));
            h.get_by_label("✏").click();
            h.run_steps(2);
            h.state_mut().rename = Some("qbit-ui-test-renomme".into());
            h.run_steps(2);
            h.get_by_label("Valider").click();
            thread::sleep(Duration::from_secs(3));
            wait(&mut h, "renommage côté serveur", |_, s| s.torrents.iter().any(|t| t.hash == hash && t.name == "qbit-ui-test-renomme"));
            shot(&mut h, "07b-details-infos");
            h.state_mut().detail_tab = DTab::Trackers;
            h.run_steps(3);
            let url = "udp://tracker.example.invalid:1337/announce";
            h.state_mut().new_trackers = url.into();
            h.run_steps(2);
            h.get_by_label("➕ Ajouter les trackers").click();
            wait(&mut h, "tracker ajouté", |_, s| s.trackers.as_ref().is_some_and(|(_, t)| t.iter().any(|t| t.url == url)));
            shot(&mut h, "07c-details-trackers");
            h.get_by_label("✖").click();
            wait(&mut h, "tracker retiré", |_, s| s.trackers.as_ref().is_some_and(|(_, t)| t.iter().all(|t| t.url != url)));
            h.state_mut().shared.lock().unwrap().detail = None;
            h.run_steps(2);
            h.get_by_label("🗑").click();
            h.run_steps(3);
            h.get_by_label("Supprimer").click();
            wait(&mut h, "suppression .torrent", |_, s| state_of(s, &hash).is_none());
        }

        if let (Ok(dir), Ok(path)) = (std::env::var("QBIT_WATCH_DIR"), std::env::var("QBIT_TORRENT")) {
            let hash = std::env::var("QBIT_TORRENT_HASH").unwrap();
            h.state_mut().s.watchers = vec![Watcher { folder: dir.clone(), save_path: "/downloads".into(), ..Default::default() }];
            let dropped = Path::new(&dir).join("depuis-firefox.torrent");
            std::fs::copy(&path, &dropped).unwrap();
            wait(&mut h, "dossier surveillé → serveur", |_, s| state_of(s, &hash).is_some());
            wait(&mut h, "fichier supprimé", |_, _| !dropped.exists());
            h.get_by_label("🗑").click();
            h.run_steps(3);
            h.get_by_label("Supprimer").click();
            wait(&mut h, "suppression torrent surveillé", |_, s| state_of(s, &hash).is_none());
        }

        h.state_mut().filter.clear();
        h.state_mut().page = Page::Settings;
        for (tab, name) in [(Tab::General, "08-param-general"), (Tab::Servers, "09-param-serveurs"), (Tab::Watch, "10-param-dossiers")] {
            h.state_mut().tab = tab;
            if tab == Tab::Watch {
                h.state_mut().s.watchers = vec![Watcher { folder: "/home/moi/Téléchargements/torrents".into(), save_path: "/downloads".into(), ..Default::default() }];
            }
            h.run_steps(3);
            shot(&mut h, name);
        }
        h.state_mut().tab = Tab::Remote;
        wait(&mut h, "préférences", |a, _| a.prefs_edit.is_some());
        shot(&mut h, "11-param-serveur");
        h.state_mut().tab = Tab::General;
        for (lang, name) in [(Lang::En, "12-en"), (Lang::Ja, "13-ja")] {
            h.state_mut().s.lang = lang;
            h.run_steps(3);
            shot(&mut h, name);
        }
        h.state_mut().page = Page::Torrents;
        h.ctx.set_theme(egui::ThemePreference::Light);
        h.run_steps(3);
        shot(&mut h, "14-liste-claire-ja");
        assert!(h.state().shared.lock().unwrap().error.is_none());
    }
}

#[cfg(test)]
mod screenshots {
    use super::*;
    use egui_kittest::Harness;
    use serde_json::json;
    use std::io::{BufRead, BufReader, Read, Write};

    const GB: i64 = 1 << 30;
    const MB: i64 = 1 << 20;

    fn hash(i: u8) -> String {
        format!("{:040x}", i as u128 * 0x1111_1111_1111)
    }

    fn torrents() -> serde_json::Value {
        let list = [
            ("TOTALLY LEGAL LINUX ISO.mp4", 4 * GB + 300 * MB, 0.63, "downloading", 8 * MB + 200_000, 120_000, 0.12, 410, "Totally legal", 41, 187),
            ("SUPER DUPER LEGALLY OBTAINED AND LEGALLY SHARED OPEN SOURCE PROJECT.mkv", 2 * GB + 900 * MB, 1.0, "uploading", 0, MB + 150_000, 4.2, 8_640_000, "Totally legal", 3, 52),
            ("GRATIS !!! FREE DOWNLOAD NO LICENSE MUSIC.mp3", 7 * MB, 0.12, "stalledDL", 0, 0, 0.0, 8_640_000, "Music", 0, 1),
            ("archlinux-2026.11.01-x86_64.iso", 1300 * MB, 0.91, "downloading", 22 * MB, 3 * MB, 0.4, 6, "Linux ISOs", 312, 40),
            ("debian-13.2.0-amd64-netinst.iso", 780 * MB, 1.0, "stoppedUP", 0, 0, 13.37, 8_640_000, "Linux ISOs", 98, 2),
            ("ubuntu-26.04-desktop-amd64.iso", 6 * GB, 0.0, "queuedDL", 0, 0, 0.0, 8_640_000, "Linux ISOs", 1204, 330),
            ("Big Buck Bunny (2008) 1080p [Creative Commons].mkv", 885 * MB, 1.0, "uploading", 0, 400_000, 2.0, 8_640_000, "", 12, 9),
            ("my grandma's cookie recipes (FINAL) (final v3) (USE THIS ONE).pdf", 18 * MB, 0.47, "stoppedDL", 0, 0, 0.0, 8_640_000, "", 1, 0),
            ("100% NOT A VIRUS free_ram_downloader_v2.zip", 66 * MB, 0.02, "error", 0, 0, 0.0, 8_640_000, "", 0, 0),
            ("Wikipedia offline dump - all of it, trust me.zim", 110 * GB, 0.0, "metaDL", 0, 0, 0.0, 8_640_000, "", 0, 4),
        ];
        list.iter()
            .enumerate()
            .map(|(i, (name, size, progress, state, dl, up, ratio, eta, cat, seeds, leechs))| {
                json!({
                    "hash": hash(i as u8 + 1), "name": name, "size": size, "progress": progress, "state": state,
                    "dlspeed": dl, "upspeed": up, "ratio": ratio, "eta": eta, "category": cat,
                    "added_on": 1_790_000_000 - i as i64 * 3600, "save_path": format!("/downloads/{}", if cat.is_empty() { "misc" } else { cat }),
                    "magnet_uri": format!("magnet:?xt=urn:btih:{}", hash(i as u8 + 1)), "num_seeds": seeds, "num_leechs": leechs,
                    "seq_dl": i == 0, "f_l_piece_prio": i == 0, "force_start": false, "dl_limit": 0, "up_limit": if i == 0 { 512 * 1024 } else { 0 },
                })
            })
            .collect()
    }

    fn respond(path: &str) -> String {
        let route = path.split('?').next().unwrap_or("").trim_start_matches("/api/v2/");
        let v = match route {
            "auth/login" => return "Ok.".into(),
            "app/defaultSavePath" => return "/downloads".into(),
            "transfer/speedLimitsMode" => return "0".into(),
            "torrents/info" => torrents(),
            "transfer/info" => json!({ "dl_info_speed": 30 * MB + 400_000, "up_info_speed": 5 * MB + 670_000 }),
            "torrents/categories" => json!({ "Linux ISOs": {}, "Music": {}, "Totally legal": {} }),
            "sync/maindata" => json!({ "server_state": { "free_space_on_disk": 2 * 1024 * GB + 600 * GB } }),
            "app/getDirectoryContent" => json!(["/downloads/Linux ISOs", "/downloads/Movies (the legal ones)", "/downloads/Music", "/downloads/Totally legal", "/downloads/misc"]),
            "torrents/files" => json!([
                { "index": 0, "name": "TOTALLY LEGAL LINUX ISO.mp4", "size": 4 * GB, "progress": 0.64, "priority": 1 },
                { "index": 1, "name": "README - please actually read me.txt", "size": 2048, "progress": 1.0, "priority": 1 },
                { "index": 2, "name": "Sample/totally-legal-sample.mp4", "size": 290 * MB, "progress": 0.5, "priority": 0 },
                { "index": 3, "name": "extras/penguin-wallpaper-4k.png", "size": 9 * MB, "progress": 1.0, "priority": 1 },
            ]),
            "torrents/trackers" => json!([
                { "url": "** [DHT] **", "status": 2, "num_seeds": 0, "num_leeches": 0, "msg": "" },
                { "url": "udp://tracker.opentrackr.org:1337/announce", "status": 2, "num_seeds": 38, "num_leeches": 171, "msg": "" },
                { "url": "udp://open.stealth.si:80/announce", "status": 3, "num_seeds": -1, "num_leeches": -1, "msg": "" },
                { "url": "https://tracker.totally-legal.example/announce", "status": 4, "num_seeds": -1, "num_leeches": -1, "msg": "Connection refused (it's fine, it's legal)" },
            ]),
            "app/preferences" => json!({ "dl_limit": 0, "up_limit": 2048 * 1024, "max_ratio_enabled": true, "max_ratio": 2.0, "max_seeding_time_enabled": false, "max_seeding_time": -1, "max_ratio_act": 1 }),
            _ => return String::new(),
        };
        v.to_string()
    }

    fn mock() -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        thread::spawn(move || {
            for s in l.incoming().flatten() {
                thread::spawn(move || {
                    let mut r = BufReader::new(&s);
                    let mut line = String::new();
                    if r.read_line(&mut line).is_err() {
                        return;
                    }
                    let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    let mut len = 0;
                    loop {
                        let mut h = String::new();
                        if r.read_line(&mut h).is_err() || h.trim().is_empty() {
                            break;
                        }
                        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0; len];
                    let _ = r.read_exact(&mut body);
                    let out = respond(&path);
                    let _ = (&s).write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len()).as_bytes());
                });
            }
        });
        format!("http://{addr}")
    }

    #[test]
    #[ignore]
    fn screenshots() {
        let out = std::env::var("QBIT_DOCS").unwrap_or("docs".into());
        let url = mock();
        for (lang, dir) in [(Lang::En, "en"), (Lang::Fr, "fr"), (Lang::Ja, "ja")] {
            let dir = format!("{out}/{dir}");
            std::fs::create_dir_all(&dir).unwrap();
            let shared = Arc::new(Mutex::new(Shared { free: -1, ..Default::default() }));
            let (wake, _woken) = mpsc::channel();
            start_background(&shared, wake, false);
            let sh = shared.clone();
            let mut h = Harness::builder().with_size([1100.0, 720.0]).wgpu().build_eframe(move |cc| App::new(cc, sh));
            ui_live::cjk(&h.ctx);
            h.ctx.set_theme(egui::ThemePreference::Dark);
            {
                let a = h.state_mut();
                a.s.lang = lang;
                a.s.servers = vec![Server {
                    name: "Seedbox (totally legal)".into(),
                    url: url.clone(),
                    user: "definitely-not-a-pirate".into(),
                    pass: "hunter2".into(),
                    ..Default::default()
                }];
                a.s.default_server = Some(0);
                a.s.watchers = vec![Watcher { folder: "~/Downloads/totally-legal-torrents".into(), save_path: "/downloads/Totally legal".into(), category: "Totally legal".into(), ..Default::default() }];
                a.page = Page::Torrents;
                a.connect(0);
            }
            ui_live::wait(&mut h, "mock", |_, s| s.loaded && s.free > 0 && !s.categories.is_empty());
            let mut frames = Vec::new();
            let mut snap = |h: &mut Harness<App>, name: &str| {
                h.run_steps(4);
                let path = format!("{dir}/{name}.png");
                h.render().unwrap().save(&path).unwrap();
                frames.push(path);
            };
            snap(&mut h, "list");
            h.state_mut().selected = [1u8, 3, 4].iter().map(|i| hash(*i)).collect();
            snap(&mut h, "selection");
            h.state_mut().selected.clear();
            h.state_mut().open_details(&hash(1));
            ui_live::wait(&mut h, "détails", |_, s| s.files.is_some() && s.trackers.is_some());
            snap(&mut h, "details-info");
            h.state_mut().detail_tab = DTab::Files;
            snap(&mut h, "details-files");
            h.state_mut().detail_tab = DTab::Trackers;
            snap(&mut h, "details-trackers");
            h.state_mut().shared.lock().unwrap().detail = None;
            h.state_mut().queue(Source::Urls("magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=linuxmint-22.3-cinnamon-64bit.iso".into()));
            snap(&mut h, "add");
            h.state_mut().open_explorer("/downloads".into(), Target::Add);
            ui_live::wait(&mut h, "explorateur", |_, s| s.dirs.as_ref().is_some_and(|(_, r)| r.is_ok()));
            snap(&mut h, "explorer");
            (h.state_mut().explorer, h.state_mut().adds) = (None, VecDeque::new());
            h.state_mut().page = Page::Settings;
            for (tab, name) in [(Tab::General, "settings-general"), (Tab::Servers, "settings-servers"), (Tab::Watch, "settings-watch"), (Tab::Remote, "settings-remote")] {
                h.state_mut().tab = tab;
                if tab == Tab::Remote {
                    ui_live::wait(&mut h, "préférences", |a, _| a.prefs_edit.is_some());
                }
                snap(&mut h, name);
            }
            h.state_mut().page = Page::Torrents;
            h.ctx.set_theme(egui::ThemePreference::Light);
            snap(&mut h, "light");
            let order = ["list", "selection", "details-info", "details-files", "details-trackers", "add", "explorer", "settings-general", "settings-watch", "light"];
            let status = std::process::Command::new("magick")
                .args(["-delay", "200", "-loop", "0"])
                .args(order.iter().map(|n| format!("{dir}/{n}.png")))
                .args(["-resize", "75%", "-layers", "Optimize", &format!("{dir}/tour.gif")])
                .status()
                .unwrap();
            assert!(status.success());
            assert!(h.state().shared.lock().unwrap().error.is_none());
        }
    }
}
