use codex_shell_command::bash::parse_shell_script_into_commands;
use std::path::Path;

pub(super) const ESCALATION_JUSTIFICATION: &str =
    "Launching a macOS application requires access to LaunchServices outside the Codex sandbox.";

/// Returns whether a plain shell script launches a macOS application through
/// LaunchServices or executes the main binary of an application bundle.
///
/// Complex scripts are left alone because this classifier must not infer an
/// executable from dynamic shell syntax. The ordinary direct-launch forms are
/// word-only commands and can be identified before Seatbelt is applied.
pub(super) fn requires_escalation(script: &str) -> bool {
    parse_shell_script_into_commands(script)
        .is_some_and(|commands| commands.iter().any(|command| is_app_launch(command)))
}

fn is_app_launch(command: &[String]) -> bool {
    let Some(executable) = command.first() else {
        return false;
    };
    let path = Path::new(executable);
    path.file_name().is_some_and(|name| name == "open") || is_app_bundle_main_executable(executable)
}

fn is_app_bundle_main_executable(executable: &str) -> bool {
    let Some((bundle_path, binary_name)) = executable.split_once(".app/Contents/MacOS/") else {
        return false;
    };
    !bundle_path.is_empty() && !binary_name.is_empty() && !binary_name.contains('/')
}

#[cfg(test)]
#[path = "macos_app_launch_tests.rs"]
mod tests;
