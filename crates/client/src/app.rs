use anyhow::{anyhow, Context, Result};
use bytes::{Bytes, BytesMut};
use common::GameInfo;
use egui::{ProgressBar, RichText, Ui};
use futures::StreamExt;
use poll_promise::Promise;
use reqwest::{cookie::Jar, Client, ClientBuilder};
use rhai::{packages::Package, Engine, Scope, AST};
use std::{
    cell::RefCell,
    fmt::Debug,
    fs,
    io::Cursor,
    path::PathBuf,
    rc::Rc,
    sync::{atomic::AtomicU64, atomic::Ordering::Relaxed, Arc},
};
use sysinfo::{
    Pid, PidExt, ProcessExt, ProcessRefreshKind, ProcessStatus, RefreshKind, System, SystemExt,
};
use tl::ParserOptions;
use zip::ZipArchive;

#[cfg(all(debug_assertions, not(feature = "prod_in_debug")))]
const SERVER_URL: &str = "http://127.0.0.1:8000";
#[cfg(any(not(debug_assertions), feature = "prod_in_debug"))]
const SERVER_URL: &str = "https://bramletts-games.shuttleapp.rs";

enum GameState {
    NotDownloaded,
    Downloading(Promise<Result<Bytes>>, Arc<(AtomicU64, AtomicU64)>),
    Downloaded(Bytes),
    Installing(Promise<Result<()>>),
    Installed,
    Running(Pid),
    // runs once; goes back to installed
    Stopped,
}

impl Debug for GameState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameState::NotDownloaded => f.debug_tuple("NotDownloaded").finish(),
            GameState::Downloading(..) => f.debug_tuple("Downloading").field(&"..").finish(),
            GameState::Downloaded(bytes) => f.debug_tuple("Downloaded").field(&bytes).finish(),
            GameState::Installing(..) => f.debug_tuple("Installing").field(&"..").finish(),
            GameState::Installed => f.debug_tuple("Installing").finish(),
            GameState::Running(pid) => f.debug_tuple("Running").field(&pid).finish(),
            GameState::Stopped => f.debug_tuple("Stopped").finish(),
        }
    }
}

pub struct Game {
    info: GameInfo,
    dir: PathBuf,
    save_dir: PathBuf,
    rhai_scope: Scope<'static>,
    hooks_ast: AST,
    state: GameState,
}

pub struct App {
    games: Vec<Game>,
    client: Client,
    rhai_engine: Engine,
    error: Rc<RefCell<Option<String>>>,
}

impl App {
    /// Called once before the first frame.
    pub async fn new(_: &eframe::CreationContext<'_>) -> Result<Self> {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.
        let client = ClientBuilder::new()
            .cookie_store(true)
            .cookie_provider(Arc::new(Jar::default()))
            .build()
            .unwrap();
        let games = client
            .get(SERVER_URL)
            .send()
            .await?
            .json::<Vec<GameInfo>>()
            .await?;
        let mut rhai_engine = Engine::new();
        rhai_fs::FilesystemPackage::new().register_into_engine(&mut rhai_engine);
        let error = Rc::new(RefCell::new(None));
        Ok(Self {
            games: match games
                .into_iter()
                .map(|info| {
                    let dir = dirs::data_local_dir().unwrap().join(info.name.clone());
                    std::fs::create_dir_all(&dir)?;
                    let save_dir = dirs::home_dir()
                        .unwrap()
                        .join("OneDrive - Brevard Public Schools")
                        .join("Saves")
                        .join(&info.name);
                    std::fs::create_dir_all(&save_dir)?;
                    let mut scope = Scope::new();
                    scope.push_constant("game_dir", dir.clone());
                    scope.push_constant("save_dir", save_dir.clone());
                    let hooks_ast = rhai_engine.compile(info.hooks.clone())?;
                    Ok(Game {
                        dir,
                        save_dir,
                        rhai_scope: scope,
                        hooks_ast,
                        info,
                        state: GameState::NotDownloaded,
                    })
                })
                .collect::<Result<Vec<Game>>>()
            {
                Ok(games) => games,
                Err(e) => {
                    error.borrow_mut().replace(e.to_string());
                    vec![]
                }
            },
            rhai_engine,
            client,
            error,
        })
    }
}

impl eframe::App for App {
    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(err) = &*self.error.borrow() {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Error (ask me about it, I'll try to help over PH):");
                ui.separator();
                ui.label(RichText::new(err).strong());
            });
            return;
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Bramlett's Totally Reliable Game Downloader");
            ui.label("Click a game to download it. Wait for it to download, then hit \"Run\".");
            ui.label("Some games may take a while to download. Please be patient.");


            let err = self.error.clone();
            for game in &mut self.games {
                ui.group(err_wrapper(err.clone(), |ui| {
                    ui.label(&game.info.name);
                    match &mut game.state {
                        GameState::NotDownloaded => {
                            if ui.button("Download").clicked() {
                                let progress = Arc::new((AtomicU64::new(0), AtomicU64::new(1)));
                                let promise = Promise::spawn_async({
                                    let client = self.client.clone();
                                    let gdrive_id = game.info.gdrive_id.clone();
                                    let progress = progress.clone();
                                    download_gdrive(gdrive_id, client, progress)
                                });
                                game.state = GameState::Downloading(promise, progress);
                            }
                        },
                        GameState::Downloading(promise, progress) => {
                            if let Some(res) = promise.ready() {
                                let bytes = res.as_ref().map_err(|e| anyhow!("{e}"))?.clone();
                                game.state = GameState::Downloaded(bytes);
                            } else {
                                let numerator = progress.0.load(Relaxed);
                                let denominator = progress.1.load(Relaxed);
                                ui
                                    .add(
                                        ProgressBar::new(
                                            numerator as f32 / denominator as f32,
                                        ).text("Downloading..."),
                                    )
                                    .on_hover_ui(|ui| {
                                        ui.label(format!("{numerator} out of {denominator} bytes downloaded"));
                                    });
                            };
                        },
                        GameState::Downloaded(bytes) => {
                            let promise = Promise::<Result<()>>::spawn_blocking({
                                let dir = game.dir.clone();
                                let bytes = bytes.clone();
                                move || extract_zip_with_password(bytes, dir, b"game")
                            });
                            game.state = GameState::Installing(promise);
                        },
                        GameState::Installing(promise) => {
                            if let Some(res) = promise.ready() {
                                res.as_ref().map_err(|e| anyhow!("{e}"))?;

                                // we run the post-install script here because the `Installed` state may be used
                                // after running a game.
                                self
                                    .rhai_engine
                                    .call_fn::<()>(&mut game.rhai_scope, &game.hooks_ast, "post_install", ())
                                    .map_err(|e| anyhow!("{e}"))?;
                                game.state = GameState::Installed;
                            } else {
                                ui.label(
                                    "Installing... (this will probably take less time than downloading, but still a while)",
                                );
                            };
                        },
                        GameState::Installed => {
                            if ui.button("Run").clicked() {
                                self
                                    .rhai_engine
                                    .call_fn::<()>(&mut game.rhai_scope, &game.hooks_ast, "pre_run", ())
                                    .map_err(|e| anyhow!("{e}"))?;
                                let pid =
                                    std::process::Command::new(game.dir.join(&game.info.exe))
                                        .current_dir(game.dir.clone())
                                        .spawn()?
                                        .id();
                                game.state = GameState::Running(Pid::from_u32(pid));
                            }
                        },
                        GameState::Running(pid) => {
                            ui.label("Running...");
                            let mut system =
                                System::new_with_specifics(
                                    RefreshKind::new().with_processes(ProcessRefreshKind::new()),
                                );
                            system.refresh_processes();
                            if ![
                                ProcessStatus::Run,
                                ProcessStatus::Sleep,
                            ].contains(&system.process(*pid).map(|p| p.status()).unwrap_or(
                                // What's here doesn't actually matter so long as it's not `Run` or `Sleep`
                                sysinfo::ProcessStatus::Zombie,
                            )) {
                                game.state = GameState::Stopped;
                            }
                        },
                        GameState::Stopped => {
                            self
                                .rhai_engine
                                .call_fn::<()>(&mut game.rhai_scope, &game.hooks_ast, "post_run", ())
                                .map_err(|e| anyhow!("{e}"))?;
                            game.state = GameState::Installed;
                        },
                    };
                    Ok(())
                }));
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                egui::warn_if_debug_build(ui);
                if !cfg!(windows) {
                    ui
                        .label(RichText::new("⚠ Not on Windows ⚠").small().color(ui.visuals().warn_fg_color))
                        .on_hover_text("Saving and some games may not work on non-Windows platforms.");
                };
            });
        });
    }
}

fn err_wrapper(
    err: Rc<RefCell<Option<String>>>,
    mut f: impl FnMut(&mut Ui) -> Result<()>,
) -> impl FnMut(&mut Ui) {
    move |ui| {
        if let Err(e) = f(ui) {
            err.borrow_mut().replace(e.to_string());
        }
    }
}

fn extract_zip_with_password(bytes: Bytes, dir: PathBuf, password: &[u8]) -> Result<()> {
    std::fs::create_dir_all(&dir)?;
    let mut archive = ZipArchive::new(Cursor::new(&bytes))?;
    for i in 0..archive.len() {
        let mut file = archive.by_index_decrypt(i, password)??;
        let mut filepath_components = file.enclosed_name().unwrap().components();
        filepath_components.next();
        let outpath = dir.join(filepath_components.as_path());
        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p)?;
                }
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }

        // make sure executable is executable on unix for wine users
        #[cfg(unix)]
        if let Some(ext) = outpath.extension() {
            if ext == "exe" {
                use std::os::unix::fs::PermissionsExt;

                let mut perms = fs::metadata(&outpath)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&outpath, perms)?;
            }
        }
    }
    Ok(())
}

async fn download_gdrive(
    gdrive_id: String,
    client: Client,
    progress: Arc<(AtomicU64, AtomicU64)>,
) -> Result<Bytes> {
    let gdrive_url = format!(
        "https://drive.google.com/uc?export=download&id={}",
        gdrive_id
    );

    // TODO: multithreaded download
    let response = client.get(&gdrive_url).send().await?.text().await?;
    let bad_drive_ctx =
        "This really shouldn't happen. Google Drive did something weird with their downloading system.";
    let real_url = {
        let dom = tl::parse(&response, ParserOptions::default())?;
        let parser = dom.parser();
        dom.get_element_by_id("download-form")
            .context(bad_drive_ctx)?
            .get(parser)
            .context("shouldn't happen; .get(parser)")?
            .as_tag()
            .context("shouldn't happen; .as_tag()")?
            .attributes()
            .get("action")
            .context(bad_drive_ctx)?
            .context(bad_drive_ctx)?
            .as_utf8_str()
            .replace("&amp;", "&")
    };

    log::info!("real google drive download URL: {}", real_url);

    let res = client.get(&real_url).send().await?;

    progress.1.store(res.content_length().unwrap(), Relaxed);

    let mut bytes = BytesMut::new();
    let mut stream = res.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        progress.0.fetch_add(chunk.len() as u64, Relaxed);
        bytes.extend_from_slice(&chunk);
    }

    Ok(bytes.freeze())
}
