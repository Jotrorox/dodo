//! Editor project selection. Reads configuration only; never runs build commands.
use crate::{codegen, file_uri, json::Value, project};
use std::path::{Path, PathBuf};
#[derive(Default)]
pub(super) struct ProjectConfig {
    pub path: Option<PathBuf>,
    required: bool,
    build_target: Option<String>,
    target: Option<String>,
}
impl ProjectConfig {
    pub fn from_initialize(params: &Value) -> Result<Self, String> {
        let options = &params["initializationOptions"];
        let string = |key: &str| -> Result<Option<String>, String> {
            match options.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) if !s.is_empty() && !s.contains('\0') => Ok(Some(s.clone())),
                _ => Err(format!("{key} must be a nonempty string")),
            }
        };
        let root = match params.get("rootUri") {
            Some(Value::String(uri)) => Some(file_uri::to_path(uri).map_err(str::to_owned)?),
            Some(Value::Null) | None => {
                match params["workspaceFolders"]
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|v| v["uri"].as_str())
                {
                    Some(uri) => Some(file_uri::to_path(uri).map_err(str::to_owned)?),
                    None => None,
                }
            }
            _ => return Err("rootUri must be a file URI or null".into()),
        };
        let manifest = string("manifestPath")?;
        let path = if let Some(path) = &manifest {
            let base = root
                .clone()
                .unwrap_or(std::env::current_dir().map_err(|e| e.to_string())?);
            Some(project::absolute(Path::new(path), &base))
        } else {
            root.map(|p| p.join("dodo.toml"))
        };
        Ok(Self {
            path,
            required: manifest.is_some(),
            build_target: string("buildTarget")?,
            target: string("target")?,
        })
    }
    pub fn settings(&self) -> Result<(String, u32), String> {
        let manifest = if let Some(path) = &self.path {
            match std::fs::symlink_metadata(path) {
                Ok(_) => Some(project::Manifest::read(path)?),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && !self.required => None,
                Err(e) => return Err(format!("cannot read manifest {}: {e}", path.display())),
            }
        } else {
            None
        };
        let host = codegen::TargetMachine::get_default_triple()
            .as_str()
            .to_string_lossy()
            .into_owned();
        let overrides = project::Settings {
            triple: self.target.clone(),
            ..Default::default()
        };
        let resolved = project::resolve(project::Request {
            manifest: manifest.as_ref(),
            target: self.build_target.as_deref(),
            profile: None,
            purpose: project::Purpose::Check,
            overrides: &overrides,
            link_args: &[],
            clear_link_args: false,
            env_linker: None,
            cwd: Path::new("."),
            host: &host,
        })?;
        let settings = resolved.settings;
        let bits = codegen::pointer_bits(&codegen::Options {
            target: settings.triple.clone(),
            cpu: settings.cpu,
            features: settings.features.unwrap_or_default(),
            ..Default::default()
        })
        .map_err(|e| format!("invalid target: {e}"))?;
        Ok((settings.triple.unwrap(), bits))
    }
}
