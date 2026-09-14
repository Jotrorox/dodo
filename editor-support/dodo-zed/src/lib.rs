use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

struct DodoExtension;

impl zed::Extension for DodoExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let settings = LspSettings::for_worktree(language_server_id.as_ref(), worktree)?;
        server_command(settings, || worktree.which("dodo"))
    }

    fn language_server_initialization_options(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        Ok(
            LspSettings::for_worktree(language_server_id.as_ref(), worktree)?
                .initialization_options,
        )
    }
}

fn server_command(
    settings: LspSettings,
    find_dodo: impl FnOnce() -> Option<String>,
) -> Result<zed::Command> {
    let binary = settings.binary.unwrap_or(zed::settings::CommandSettings {
        path: None,
        arguments: None,
        env: None,
    });
    let command = binary.path.or_else(find_dodo).ok_or_else(|| {
        "Dodo was not found on PATH. Install Dodo 0.1.2 or newer, or set lsp.dodo.binary.path in Zed settings to the compiler executable.".to_string()
    })?;

    Ok(zed::Command {
        command,
        args: binary.arguments.unwrap_or_else(|| vec!["lsp".into()]),
        env: binary.env.unwrap_or_default().into_iter().collect(),
    })
}

zed::register_extension!(DodoExtension);

#[cfg(test)]
mod tests {
    use super::*;
    use zed::serde_json::{from_value, json};

    #[test]
    fn discovers_compiler_and_supplies_lsp_subcommand() {
        let command =
            server_command(LspSettings::default(), || Some("/usr/bin/dodo".into())).unwrap();
        assert_eq!(command.command, "/usr/bin/dodo");
        assert_eq!(command.args, ["lsp"]);
        assert!(command.env.is_empty());
    }

    #[test]
    fn configured_path_with_spaces_does_not_require_path_discovery() {
        let settings =
            from_value(json!({"binary": {"path": "/tools/Dodo Compiler/dodo"}})).unwrap();
        let command = server_command(settings, || panic!("must not probe PATH")).unwrap();
        assert_eq!(command.command, "/tools/Dodo Compiler/dodo");
        assert_eq!(command.args, ["lsp"]);
    }

    #[test]
    fn wrapper_arguments_and_environment_are_preserved() {
        let settings = from_value(json!({"binary": {
            "path": "/tools/wrapper",
            "arguments": ["--compiler", "/tools/Dodo Compiler/dodo", "lsp"],
            "env": {"DODO_LOG": "debug"}
        }}))
        .unwrap();
        let command = server_command(settings, || None).unwrap();
        assert_eq!(
            command.args,
            ["--compiler", "/tools/Dodo Compiler/dodo", "lsp"]
        );
        assert_eq!(command.env, [("DODO_LOG".into(), "debug".into())]);

        let settings = from_value(json!({"binary": {"arguments": []}})).unwrap();
        let command = server_command(settings, || Some("/tools/lsp-wrapper".into())).unwrap();
        assert!(command.args.is_empty());
    }

    #[test]
    fn missing_compiler_explains_how_to_configure_it() {
        let error = server_command(LspSettings::default(), || None).unwrap_err();
        assert!(error.contains("lsp.dodo.binary.path"));
    }
}
