//! Interface : quatre écrans dans l'ordre (image, vérification, clé,
//! écriture), tâches longues en arrière-plan, journal repliable.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use secmet_attest::release::LatestRelease;

use crate::disks::{self, Disk};
use crate::image::{self, ImageInfo, Verified};
use crate::release;
use crate::worker::{Event, Phase};
use crate::{fmt_size, fmt_size_decimal, platform};

const TITLE: &str = "SecMet Clé USB";
const GREEN: egui::Color32 = egui::Color32::from_rgb(46, 125, 50);

/// Lance l'interface, sur `image` si elle est donnée.
pub fn run(image: Option<PathBuf>) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size([660.0, 540.0])
            .with_min_inner_size([540.0, 440.0]),
        ..Default::default()
    };
    eframe::run_native(
        TITLE,
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_zoom_factor(1.1);
            let mut app = App::new();
            if let Some(p) = image {
                app.choose_local(&p);
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| format!("interface graphique : {e}"))
}

/// Tâche d'arrière-plan avec progression `(fait, total)` et résultat.
struct Task<T> {
    progress: Arc<Mutex<(u64, u64)>>,
    result: Arc<Mutex<Option<Result<T, String>>>>,
    cancel: Arc<AtomicBool>,
    started: Instant,
}

impl<T: Send + 'static> Task<T> {
    fn spawn<F>(f: F) -> Self
    where
        F: FnOnce(&AtomicBool, &mut dyn FnMut(u64, u64)) -> Result<T, String> + Send + 'static,
    {
        let progress = Arc::new(Mutex::new((0u64, 0u64)));
        let result = Arc::new(Mutex::new(None));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p, r, c) = (progress.clone(), result.clone(), cancel.clone());
        std::thread::spawn(move || {
            let mut report = |done: u64, total: u64| {
                if let Ok(mut g) = p.lock() {
                    *g = (done, total);
                }
            };
            let out = f(&c, &mut report);
            if let Ok(mut g) = r.lock() {
                *g = Some(out);
            }
        });
        Task {
            progress,
            result,
            cancel,
            started: Instant::now(),
        }
    }

    fn progress(&self) -> (u64, u64) {
        self.progress.lock().map(|g| *g).unwrap_or((0, 0))
    }

    fn take_result(&self) -> Option<Result<T, String>> {
        self.result.lock().ok().and_then(|mut g| g.take())
    }

    fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Source,
    Verify,
    Disk,
    Confirm,
    Write,
    Done,
    Failed,
}

impl Step {
    fn index(self) -> usize {
        match self {
            Step::Source => 0,
            Step::Verify => 1,
            Step::Disk | Step::Confirm => 2,
            Step::Write | Step::Done | Step::Failed => 3,
        }
    }
}

/// Notification système de fin (succès ou échec), pour que le résultat
/// soit vu même si la fenêtre est derrière une autre.
fn notify(ctx: &egui::Context, ok: bool, title: &str, body: &str) {
    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
        egui::UserAttentionType::Critical,
    ));
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("notify-send")
            .arg("--app-name=SecMet Clé USB")
            .arg(format!(
                "--icon={}",
                if ok {
                    "emblem-ok-symbolic"
                } else {
                    "dialog-error"
                }
            ))
            .arg(if ok {
                "--urgency=normal"
            } else {
                "--urgency=critical"
            })
            .arg(title)
            .arg(body)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (ok, title, body);
}

/// Image choisie.
#[derive(Debug, Clone)]
struct Source {
    path: PathBuf,
    version: Option<String>,
}

/// État partagé avec le lecteur de la sortie du processus d'écriture.
#[derive(Default)]
struct WriteState {
    phase: Option<Phase>,
    done: u64,
    total: u64,
    phase_started: Option<Instant>,
    log: Vec<String>,
    result: Option<Result<(String, u64), String>>,
}

struct WriteJob {
    child: Child,
    stdin: Option<ChildStdin>,
    state: Arc<Mutex<WriteState>>,
    started: Instant,
    temp: Option<PathBuf>,
    cancelled: bool,
}

impl Drop for WriteJob {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
        if let Some(t) = &self.temp {
            let _ = std::fs::remove_file(t);
        }
    }
}

struct App {
    step: Step,
    latest: Option<Task<LatestRelease>>,
    latest_result: Option<Result<LatestRelease, String>>,
    source: Option<Source>,
    download: Option<Task<PathBuf>>,
    verify: Option<Task<(Verified, ImageInfo)>>,
    verified: Option<(Verified, ImageInfo)>,
    error: Option<String>,
    disks: Vec<Disk>,
    disks_error: Option<String>,
    disks_refreshed: Option<Instant>,
    selected: Option<String>,
    confirmed: bool,
    write: Option<WriteJob>,
    write_summary: Option<(String, u64, Duration)>,
    log: Vec<String>,
    show_log: bool,
    /// Notification à envoyer au prochain rendu : (succès, titre, texte).
    pending_notice: Option<(bool, String, String)>,
}

impl App {
    fn new() -> Self {
        let mut app = App {
            step: Step::Source,
            latest: None,
            latest_result: None,
            source: None,
            download: None,
            verify: None,
            verified: None,
            error: None,
            disks: Vec::new(),
            disks_error: None,
            disks_refreshed: None,
            selected: None,
            confirmed: false,
            write: None,
            write_summary: None,
            log: Vec::new(),
            show_log: false,
            pending_notice: None,
        };
        app.fetch_latest();
        app
    }

    fn log(&mut self, line: impl Into<String>) {
        self.log.push(line.into());
    }

    fn fetch_latest(&mut self) {
        self.latest_result = None;
        self.latest = Some(Task::spawn(|_, _| release::fetch_latest()));
    }

    fn latest(&self) -> Option<&LatestRelease> {
        match &self.latest_result {
            Some(Ok(l)) => Some(l),
            _ => None,
        }
    }

    fn start_download(&mut self, version: &str) {
        let name = release::image_name(version);
        let url = format!("{}{name}", release::release_dir_url(version));
        let dest = release::download_dir().join(&name);
        self.log(format!("téléchargement de {url} vers {}", dest.display()));
        self.error = None;
        self.download = Some(Task::spawn(move |cancel, report| {
            release::download(&url, &dest, cancel, &mut |d, t| report(d, t.unwrap_or(0)))?;
            Ok(dest)
        }));
    }

    fn choose_local(&mut self, path: &Path) {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let version = image::version_from_name(name);
        self.log(format!("image choisie : {}", path.display()));
        self.set_source(Source {
            path: path.to_path_buf(),
            version,
        });
    }

    fn set_source(&mut self, source: Source) {
        self.source = Some(source);
        self.verified = None;
        self.error = None;
        self.step = Step::Verify;
        self.start_verify();
    }

    fn start_verify(&mut self) {
        let Some(src) = self.source.clone() else {
            return;
        };
        let latest = self.latest().cloned();
        self.error = None;
        self.verify = Some(Task::spawn(move |cancel, report| {
            image::verify_with_release(&src.path, latest.as_ref(), cancel, report)
        }));
    }

    fn refresh_disks(&mut self, force: bool) {
        let due = self
            .disks_refreshed
            .map(|t| t.elapsed() > Duration::from_secs(2))
            .unwrap_or(true);
        if !(force || due) {
            return;
        }
        self.disks_refreshed = Some(Instant::now());
        match disks::list() {
            Ok(d) => {
                self.disks = d;
                self.disks_error = None;
            }
            Err(e) => self.disks_error = Some(e),
        }
        if let Some(sel) = &self.selected {
            if !self.disks.iter().any(|d| &d.path == sel) {
                self.selected = None;
                self.confirmed = false;
            }
        }
    }

    fn selected_disk(&self) -> Option<&Disk> {
        self.selected
            .as_ref()
            .and_then(|p| self.disks.iter().find(|d| &d.path == p))
    }

    fn start_write(&mut self) {
        let (Some(src), Some(disk)) = (self.source.clone(), self.selected_disk().cloned()) else {
            return;
        };
        self.error = None;
        self.write_summary = None;
        let (mut cmd, temp) = match platform::elevated_worker(&disk.path, &src.path) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("lancement du processus d'écriture : {e}"));
                return;
            }
        };
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("lancement du processus d'écriture : {e}"));
                if let Some(t) = temp {
                    let _ = std::fs::remove_file(t);
                }
                return;
            }
        };
        self.log(format!(
            "écriture de {} sur {} ({})",
            src.path.display(),
            disk.path,
            disk.name
        ));
        let state = Arc::new(Mutex::new(WriteState::default()));
        let stdin = child.stdin.take();
        if let Some(out) = child.stdout.take() {
            let st = state.clone();
            std::thread::spawn(move || read_events(out, st));
        }
        if let Some(err) = child.stderr.take() {
            let st = state.clone();
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = BufReader::new(err).read_to_string(&mut text);
                if let Ok(mut g) = st.lock() {
                    for l in text.lines().filter(|l| !l.trim().is_empty()) {
                        g.log.push(format!("[système] {l}"));
                    }
                }
            });
        }
        self.write = Some(WriteJob {
            child,
            stdin,
            state,
            started: Instant::now(),
            temp,
            cancelled: false,
        });
        self.step = Step::Write;
    }

    /// Fait avancer les tâches ; vrai s'il faut rafraîchir bientôt.
    fn poll(&mut self) -> bool {
        let mut busy = false;
        if let Some(t) = &self.latest {
            if let Some(r) = t.take_result() {
                match &r {
                    Ok(l) => self.log(format!("dernière version publiée : {}", l.version)),
                    Err(e) => self.log(format!("version courante inconnue : {e}")),
                }
                self.latest_result = Some(r);
                self.latest = None;
            } else {
                busy = true;
            }
        }
        if let Some(t) = &self.download {
            if let Some(r) = t.take_result() {
                self.download = None;
                match r {
                    Ok(path) => {
                        let version = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .and_then(image::version_from_name);
                        self.log(format!("téléchargé : {}", path.display()));
                        self.set_source(Source { path, version });
                    }
                    Err(e) => {
                        self.log(format!("téléchargement : {e}"));
                        self.error = Some(e);
                    }
                }
            } else {
                busy = true;
            }
        }
        if let Some(t) = &self.verify {
            if let Some(r) = t.take_result() {
                self.verify = None;
                match r {
                    Ok((v, info)) => {
                        self.log(format!(
                            "signature valide (clé de version {}), SHA-256 {} conforme ; {} compressés, {} à écrire",
                            v.key_id,
                            v.sha256,
                            fmt_size(info.compressed_size),
                            fmt_size(info.uncompressed_size)
                        ));
                        self.verified = Some((v, info));
                        self.step = Step::Disk;
                        self.refresh_disks(true);
                    }
                    Err(e) => {
                        self.log(format!("vérification : {e}"));
                        self.error = Some(e);
                    }
                }
            } else {
                busy = true;
            }
        }
        if self.step == Step::Disk || self.step == Step::Confirm {
            self.refresh_disks(false);
            busy = true;
        }
        if let Some(job) = &mut self.write {
            let finished = job.child.try_wait().ok().flatten();
            let result = job.state.lock().ok().and_then(|g| g.result.clone());
            if let Some(r) = result {
                let elapsed = job.started.elapsed();
                let lines: Vec<String> = job
                    .state
                    .lock()
                    .map(|mut g| std::mem::take(&mut g.log))
                    .unwrap_or_default();
                self.write = None;
                self.log.extend(lines);
                match r {
                    Ok((sha, written)) => {
                        self.log(format!(
                            "clé écrite et relue : {} , SHA-256 {sha}",
                            fmt_size(written)
                        ));
                        self.write_summary = Some((sha, written, elapsed));
                        self.step = Step::Done;
                    }
                    Err(e) => {
                        self.log(format!("échec : {e}"));
                        self.error = Some(e);
                        self.step = Step::Confirm;
                        self.confirmed = false;
                    }
                }
            } else if let Some(status) = finished {
                // Terminé sans message final : pkexec refusé, ou tué.
                std::thread::sleep(Duration::from_millis(50));
                let lines: Vec<String> = job
                    .state
                    .lock()
                    .map(|mut g| std::mem::take(&mut g.log))
                    .unwrap_or_default();
                let cancelled = job.cancelled;
                self.write = None;
                self.log.extend(lines);
                let msg = if cancelled {
                    "écriture annulée : la clé est incomplète, recommencer".to_string()
                } else {
                    platform::elevation_hint(status.code())
                };
                self.log(format!("échec : {msg}"));
                self.pending_notice = Some((false, "Clé non écrite".to_string(), msg.clone()));
                self.error = Some(msg);
                self.step = Step::Failed;
                self.confirmed = false;
            } else {
                busy = true;
            }
        }
        busy
    }

    fn reset_to_source(&mut self) {
        if let Some(t) = &self.download {
            t.cancel();
        }
        if let Some(t) = &self.verify {
            t.cancel();
        }
        self.download = None;
        self.verify = None;
        self.source = None;
        self.verified = None;
        self.error = None;
        self.selected = None;
        self.confirmed = false;
        self.step = Step::Source;
    }
}

fn read_events(out: std::process::ChildStdout, state: Arc<Mutex<WriteState>>) {
    for line in BufReader::new(out).lines() {
        let Ok(line) = line else { break };
        let Ok(mut g) = state.lock() else { break };
        match serde_json::from_str::<Event>(&line) {
            Ok(Event::Phase { phase, total }) => {
                g.phase = Some(phase);
                g.total = total;
                g.done = 0;
                g.phase_started = Some(Instant::now());
                g.log.push(format!("étape : {}", phase.label()));
            }
            Ok(Event::Progress { done }) => g.done = done,
            Ok(Event::Info { message }) => g.log.push(message),
            Ok(Event::Done { sha256, written }) => g.result = Some(Ok((sha256, written))),
            Ok(Event::Error { message }) => g.result = Some(Err(message)),
            Err(_) => g.log.push(format!("[sortie] {line}")),
        }
    }
}

fn speed_eta(done: u64, total: u64, since: Option<Instant>) -> String {
    let Some(start) = since else {
        return String::new();
    };
    let secs = start.elapsed().as_secs_f64();
    if secs < 1.0 || done == 0 {
        return String::new();
    }
    let rate = done as f64 / secs;
    let mut s = format!("{:.0} Mio/s", rate / (1024.0 * 1024.0));
    if total > done && rate > 0.0 {
        let remaining = ((total - done) as f64 / rate).round() as u64;
        s.push_str(&format!(
            " , reste {}",
            fmt_duration(Duration::from_secs(remaining))
        ));
    }
    s
}

fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 60 {
        format!("{} min {:02} s", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let busy = self.poll();
        if busy {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
        if let Some((ok, title, body)) = self.pending_notice.take() {
            notify(&ctx, ok, &title, &body);
        }
        if self.step == Step::Source {
            let dropped: Vec<PathBuf> = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .map(|f| f.path().to_path_buf())
                    .collect()
            });
            if let Some(p) = dropped.first() {
                self.choose_local(p);
            }
        }

        egui::Panel::top("steps").show(root, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let names = ["1. Image", "2. Vérification", "3. Clé USB", "4. Écriture"];
                let current = self.step.index();
                for (i, name) in names.iter().enumerate() {
                    if i > 0 {
                        ui.weak("›");
                    }
                    if i == current {
                        ui.label(egui::RichText::new(*name).strong());
                    } else if i < current {
                        ui.colored_label(GREEN, *name);
                    } else {
                        ui.weak(*name);
                    }
                }
            });
            ui.add_space(6.0);
        });

        egui::Panel::bottom("foot").show(root, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.toggle_value(&mut self.show_log, "Détails");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(format!("SecMet Clé USB {}", env!("CARGO_PKG_VERSION")));
                });
            });
            if self.show_log {
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for l in &self.log {
                            ui.small(l);
                        }
                        if let Some(job) = &self.write {
                            if let Ok(g) = job.state.lock() {
                                for l in &g.log {
                                    ui.small(l);
                                }
                            }
                        }
                    });
            }
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                match self.step {
                    Step::Source => self.ui_source(ui),
                    Step::Verify => self.ui_verify(ui),
                    Step::Disk => self.ui_disk(ui),
                    Step::Confirm => self.ui_confirm(ui),
                    Step::Write => self.ui_write(ui),
                    Step::Done => self.ui_done(ui),
                    Step::Failed => self.ui_failed(ui),
                }
            });
        });
    }
}

impl App {
    fn ui_error(&self, ui: &mut egui::Ui) {
        if let Some(e) = &self.error {
            ui.add_space(6.0);
            ui.colored_label(ui.visuals().error_fg_color, format!("✗ {e}"));
            ui.add_space(6.0);
        }
    }

    fn ui_source(&mut self, ui: &mut egui::Ui) {
        ui.heading("Image de la clé SecMet");
        ui.label("La clé USB SecMet est fabriquée à partir de l'image publiée avec chaque version. Tout le contenu de la clé choisie sera effacé.");
        ui.add_space(10.0);

        if let Some(t) = &self.download {
            let (done, total) = t.progress();
            ui.group(|ui| {
                ui.label(egui::RichText::new("Téléchargement en cours").strong());
                let frac = if total > 0 {
                    done as f32 / total as f32
                } else {
                    0.0
                };
                let text = if total > 0 {
                    format!(
                        "{} / {}  {}",
                        fmt_size(done),
                        fmt_size(total),
                        speed_eta(done, total, Some(t.started))
                    )
                } else {
                    fmt_size(done)
                };
                ui.add(egui::ProgressBar::new(frac).text(text).animate(total == 0));
                if ui.button("Annuler").clicked() {
                    t.cancel();
                }
            });
            return;
        }

        ui.group(|ui| {
            ui.label(egui::RichText::new("Télécharger la dernière version").strong());
            match (&self.latest_result, &self.latest) {
                (None, Some(_)) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Recherche de la version publiée…");
                    });
                }
                (Some(Err(e)), _) => {
                    ui.colored_label(ui.visuals().error_fg_color, format!("Version publiée inaccessible : {e}"));
                    if ui.button("Réessayer").clicked() {
                        self.fetch_latest();
                    }
                }
                (Some(Ok(l)), _) => {
                    let version = l.version.clone();
                    let name = release::image_name(&version);
                    let dest = release::download_dir().join(&name);
                    ui.label(format!("SecMet {version}, publiée le {} ; environ 850 Mo à télécharger, signée par la clé de version.", l.published_at));
                    ui.horizontal(|ui| {
                        if dest.is_file() {
                            if ui.button(format!("Utiliser {}", dest.display())).clicked() {
                                self.choose_local(&dest);
                            }
                            if ui.button("Télécharger à nouveau").clicked() {
                                let _ = std::fs::remove_file(&dest);
                                self.start_download(&version);
                            }
                        } else if ui.button(format!("Télécharger dans {}", release::download_dir().display())).clicked() {
                            self.start_download(&version);
                        }
                    });
                }
                (None, None) => {}
            }
        });
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label(egui::RichText::new("Ou utiliser un fichier déjà téléchargé").strong());
            ui.label("secmet-key_<version>_x86_64.img.xz, depuis la page des versions ou une autre machine. Vous pouvez aussi le déposer dans cette fenêtre.");
            if ui.button("Choisir le fichier…").clicked() {
                let picked = rfd::FileDialog::new()
                    .set_title("Image de la clé SecMet")
                    .add_filter("Image de clé SecMet (.img.xz)", &["xz"])
                    .set_directory(release::download_dir())
                    .pick_file();
                if let Some(p) = picked {
                    self.choose_local(&p);
                }
            }
        });
        self.ui_error(ui);
    }

    fn ui_verify(&mut self, ui: &mut egui::Ui) {
        ui.heading("Vérification de l'image");
        if let Some(src) = &self.source {
            ui.label(format!("Fichier : {}", src.path.display()));
            match (&src.version, self.latest()) {
                (Some(v), Some(l)) if secmet_attest::release::is_newer(&l.version, v) => {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!(
                            "Cette image est la version {v} ; la version {} est publiée.",
                            l.version
                        ),
                    );
                }
                _ => {}
            }
        }
        ui.add_space(8.0);
        if let Some(t) = &self.verify {
            let (done, total) = t.progress();
            ui.label("Signature de la release avec la clé de version SecMet, puis empreinte SHA-256 du fichier.");
            let frac = if total > 0 {
                done as f32 / total as f32
            } else {
                0.0
            };
            ui.add(
                egui::ProgressBar::new(frac)
                    .text(if total > 0 {
                        format!("empreinte : {} / {}", fmt_size(done), fmt_size(total))
                    } else {
                        "lecture de la signature…".to_string()
                    })
                    .animate(total == 0),
            );
            if ui.button("Annuler").clicked() {
                self.reset_to_source();
            }
            return;
        }
        self.ui_error(ui);
        if self.error.is_some() {
            ui.horizontal(|ui| {
                if ui.button("Réessayer").clicked() {
                    self.start_verify();
                }
                if ui.button("Autre image").clicked() {
                    self.reset_to_source();
                }
            });
        }
    }

    fn ui_image_summary(&self, ui: &mut egui::Ui) {
        if let (Some(src), Some((v, info))) = (&self.source, &self.verified) {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(GREEN, "✓");
                ui.label(format!(
                    "SecMet {} : signature valide (clé de version {}), empreinte conforme, {} à écrire.",
                    src.version.as_deref().unwrap_or("?"),
                    v.key_id,
                    fmt_size(info.uncompressed_size)
                ));
            });
        }
    }

    fn ui_disk(&mut self, ui: &mut egui::Ui) {
        ui.heading("Choisir la clé USB");
        self.ui_image_summary(ui);
        ui.label("Seules les clés et disques amovibles de 12 Go ou plus sont proposés ; les disques internes n'apparaissent jamais.");
        ui.add_space(8.0);
        let needed = self
            .verified
            .as_ref()
            .map(|(_, i)| i.uncompressed_size)
            .unwrap_or(0);
        if let Some(e) = &self.disks_error {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        let candidates: Vec<Disk> = self.disks.iter().filter(|d| d.removable).cloned().collect();
        if candidates.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Aucune clé USB détectée. Branchez la clé : la liste se met à jour toute seule.");
            });
        } else {
            egui::Grid::new("disks")
                .num_columns(4)
                .striped(true)
                .spacing([14.0, 8.0])
                .show(ui, |ui| {
                    for d in &candidates {
                        let refusal = d.refusal(needed);
                        let is_sel = self.selected.as_deref() == Some(d.path.as_str());
                        ui.add_enabled_ui(refusal.is_none(), |ui| {
                            if ui.radio(is_sel, "").clicked() {
                                self.selected = Some(d.path.clone());
                                self.confirmed = false;
                            }
                        });
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(&d.name).strong());
                            ui.weak(format!(
                                "{}  ·  série {}",
                                d.path,
                                d.serial.as_deref().unwrap_or("inconnue")
                            ));
                        });
                        ui.label(fmt_size_decimal(d.size));
                        match refusal {
                            Some(r) => {
                                ui.colored_label(ui.visuals().warn_fg_color, r);
                            }
                            None if !d.mounted.is_empty() => {
                                ui.weak(format!(
                                    "montée ({}), sera démontée",
                                    d.mounted.join(", ")
                                ));
                            }
                            None => {
                                ui.label("");
                            }
                        }
                        ui.end_row();
                    }
                });
        }
        self.ui_error(ui);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let ok = self
                .selected_disk()
                .map(|d| d.refusal(needed).is_none())
                .unwrap_or(false);
            if ui.add_enabled(ok, egui::Button::new("Continuer")).clicked() {
                self.error = None;
                self.step = Step::Confirm;
            }
            if ui.button("Autre image").clicked() {
                self.reset_to_source();
            }
        });
    }

    fn ui_confirm(&mut self, ui: &mut egui::Ui) {
        ui.heading("Confirmer l'effacement");
        self.ui_image_summary(ui);
        let Some(d) = self.selected_disk().cloned() else {
            self.step = Step::Disk;
            return;
        };
        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label(
                egui::RichText::new(format!("{}  ·  {}", d.name, fmt_size_decimal(d.size)))
                    .strong()
                    .size(16.0),
            );
            ui.label(format!(
                "{}  ·  série {}",
                d.path,
                d.serial.as_deref().unwrap_or("inconnue")
            ));
        });
        ui.add_space(8.0);
        ui.colored_label(
            ui.visuals().warn_fg_color,
            "Tout le contenu de ce disque sera définitivement effacé. L'écriture puis la relecture prennent 3 à 5 minutes en USB 3, 15 à 20 en USB 2.",
        );
        if cfg!(target_os = "linux") && !platform::is_privileged() {
            ui.weak(
                "Le système demandera votre mot de passe pour autoriser l'écriture sur le disque.",
            );
        }
        ui.add_space(6.0);
        ui.checkbox(
            &mut self.confirmed,
            "J'ai vérifié qu'il s'agit de la bonne clé",
        );
        self.ui_error(ui);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.confirmed,
                    egui::Button::new("Effacer et écrire la clé"),
                )
                .clicked()
            {
                self.start_write();
            }
            if ui.button("Retour").clicked() {
                self.error = None;
                self.step = Step::Disk;
            }
        });
    }

    fn ui_write(&mut self, ui: &mut egui::Ui) {
        ui.heading("Écriture de la clé");
        self.ui_image_summary(ui);
        if let Some(d) = self.selected_disk() {
            ui.label(format!(
                "Vers {} ({}, {})",
                d.name,
                d.path,
                fmt_size_decimal(d.size)
            ));
        }
        ui.add_space(10.0);
        let mut cancel_clicked = false;
        if let Some(job) = &self.write {
            let (phase, done, total, since) = job
                .state
                .lock()
                .map(|g| (g.phase, g.done, g.total, g.phase_started))
                .unwrap_or((None, 0, 0, None));
            let phases = [
                Phase::Prepare,
                Phase::Write,
                Phase::Flush,
                Phase::Verify,
                Phase::Finish,
            ];
            let current = phase.map(|p| phases.iter().position(|q| *q == p).unwrap_or(0));
            for (i, p) in phases.iter().enumerate() {
                ui.horizontal(|ui| match current {
                    Some(c) if i < c => {
                        ui.colored_label(GREEN, "✓");
                        ui.label(p.label());
                    }
                    Some(c) if i == c => {
                        ui.spinner();
                        ui.label(egui::RichText::new(p.label()).strong());
                    }
                    _ => {
                        ui.weak("·");
                        ui.weak(p.label());
                    }
                });
                if current == Some(i) && total > 0 {
                    let frac = done as f32 / total as f32;
                    ui.add(egui::ProgressBar::new(frac).text(format!(
                        "{} / {}  {}",
                        fmt_size(done),
                        fmt_size(total),
                        speed_eta(done, total, since)
                    )));
                }
            }
            if current.is_none() {
                ui.weak("En attente de l'autorisation…");
            }
            ui.add_space(8.0);
            ui.weak("Ne débranchez pas la clé.");
            ui.add_space(8.0);
            if ui
                .add_enabled(!job.cancelled, egui::Button::new("Annuler"))
                .clicked()
            {
                cancel_clicked = true;
            }
        }
        if cancel_clicked {
            if let Some(job) = &mut self.write {
                job.cancelled = true;
                drop(job.stdin.take());
            }
            self.log("annulation demandée");
        }
    }

    fn ui_failed(&mut self, ui: &mut egui::Ui) {
        ui.heading("Clé non écrite");
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                ui.visuals().error_fg_color,
                egui::RichText::new("✗").size(22.0),
            );
            ui.label(
                egui::RichText::new(self.error.clone().unwrap_or_default())
                    .strong()
                    .size(15.0),
            );
        });
        ui.add_space(8.0);
        ui.label("La clé n'est pas utilisable en l'état : elle est vide ou incomplète. Rien n'a été écrit ailleurs. Le journal (« Détails », en bas) contient le déroulé complet.");
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Réessayer sur la même clé").clicked() {
                self.error = None;
                self.step = Step::Confirm;
            }
            if ui.button("Choisir une autre clé").clicked() {
                self.error = None;
                self.selected = None;
                self.step = Step::Disk;
                self.refresh_disks(true);
            }
            if ui.button("Quitter").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    fn ui_done(&mut self, ui: &mut egui::Ui) {
        ui.heading("Clé prête");
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(GREEN, egui::RichText::new("✓").size(26.0));
            if let Some((sha, written, elapsed)) = &self.write_summary {
                ui.label(
                    egui::RichText::new(format!(
                        "Écriture terminée et vérifiée : {} écrits puis relus à l'identique en {}.",
                        fmt_size(*written),
                        fmt_duration(*elapsed)
                    ))
                    .strong()
                    .size(15.0),
                );
                ui.weak(format!("SHA-256 des données écrites : {sha}"));
            }
        });
        ui.add_space(10.0);
        ui.label("Vous pouvez débrancher la clé. Au premier démarrage sur un PC (menu de démarrage : F12, F9 ou Échap selon la marque), la clé crée sa clé de signature des certificats et agrandit sa partition d'export ; cela prend quelques secondes avant le bureau.");
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Écrire une autre clé").clicked() {
                self.selected = None;
                self.confirmed = false;
                self.write_summary = None;
                self.step = Step::Disk;
                self.refresh_disks(true);
            }
            if ui.button("Quitter").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}
