use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use color_eyre::{
    eyre::{ensure, Context},
    Result,
};
use hotwatch::{Event, Hotwatch};
use log::error;
use serde::Deserialize;
use smithay_client_toolkit::reexports::calloop::channel::Sender;

use crate::wallpaper_info::WallpaperInfo;

#[derive(Deserialize)]
pub struct WallpapersConfig {
    #[serde(flatten)]
    data: HashMap<String, Arc<WallpaperInfo>>,
    #[serde(skip)]
    default_config: Arc<WallpaperInfo>,
    #[serde(skip)]
    pub path: PathBuf,
    #[serde(skip)]
    pub reloaded: Option<Arc<AtomicBool>>,
}

impl WallpapersConfig {
    pub fn new_from_path(path: &Path) -> Result<Self> {
        ensure!(path.exists(), "Configuration file {path:?} does not exists",);
        let mut config_manager: Self = toml::from_str(&fs::read_to_string(path)?)?;
        config_manager.default_config = config_manager
            .data
            .get("default")
            .unwrap_or(&Arc::new(WallpaperInfo::default()))
            .clone();
        for (name, config) in &config_manager.data {
            let path = config.path.as_ref().unwrap();
            ensure!(
                path.exists(),
                "File or directory {path:?} for input {name} does not exist"
            );
            ensure!(
                config.duration.is_none() || path.is_dir(),
                "for input '{name}', `path` is set to an image but `duration` is also set.
Either remove `duration` or set `path` to a directory"
            );
        }

        config_manager.path = path.to_path_buf();
        Ok(config_manager)
    }

    pub fn get_output_by_name(&self, name: &str) -> Arc<WallpaperInfo> {
        self.data.get(name).unwrap_or(&self.default_config).clone()
    }

    pub fn listen_to_changes(&self, hotwatch: &mut Hotwatch, ev_tx: Sender<()>) -> Result<()> {
        let reloaded = self.reloaded.as_ref().unwrap().clone();
        hotwatch
            .watch(&self.path, move |event: Event| {
                if let hotwatch::EventKind::Modify(_) = event.kind {
                    reloaded.store(true, Ordering::Relaxed);
                    ev_tx.send(()).unwrap();
                }
            })
            .with_context(|| format!("watching file {:?}", &self.path))?;
        Ok(())
    }

    pub fn paths(&self) -> Vec<&PathBuf> {
        let mut paths: Vec<_> = self
            .data
            .values()
            .filter_map(|info| info.path.as_ref())
            .collect();
        paths.sort_unstable();
        paths.dedup();
        paths
    }

    /// Return true if the struct changed
    pub(crate) fn try_update(&mut self) -> bool {
        // When the config file has been written into
        let mut new_config = WallpapersConfig::new_from_path(&self.path)
            .with_context(|| format!("reading configuration from file {:?}", self.path));
        match new_config {
            Ok(new_config) if new_config != *self => {
                let reloaded = self.reloaded.as_ref().unwrap().clone();
                *self = new_config;
                self.reloaded = Some(reloaded);
                true
            }
            Ok(_) => {
                // Do nothing, the new config is the same as the loaded one
                false
            }
            Err(err) => {
                error!("{:?}", err);
                false
            }
        }
    }
}

impl PartialEq for WallpapersConfig {
    fn eq(&self, other: &Self) -> bool {
        self.data == other.data
    }
}
