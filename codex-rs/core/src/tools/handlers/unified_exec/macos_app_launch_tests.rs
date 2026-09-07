use super::requires_escalation;
use pretty_assertions::assert_eq;

#[test]
fn identifies_direct_app_bundle_launches() {
    let scripts = [
        r#"'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' --headless=new about:blank"#,
        r#"./target/debug/Example.app/Contents/MacOS/Example --test"#,
        r#"echo preparing && '/Applications/LibreOffice.app/Contents/MacOS/soffice' --headless"#,
    ];

    assert_eq!(scripts.map(requires_escalation), [true, true, true]);
}

#[test]
fn identifies_launch_services_open_commands() {
    let scripts = [
        r#"open -a 'Google Chrome' about:blank"#,
        r#"/usr/bin/open -n /Applications/TextEdit.app"#,
    ];

    assert_eq!(scripts.map(requires_escalation), [true, true]);
}

#[test]
fn ignores_non_launch_commands_and_dynamic_scripts() {
    let scripts = [
        r#"printf '%s\n' open"#,
        r#"echo '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'"#,
        r#"/Applications/Example.app/Contents/Resources/helper"#,
        r#"/Applications/Example.app/Contents/MacOS/"#,
        r#"if true; then open -a TextEdit; fi"#,
    ];

    assert_eq!(
        scripts.map(requires_escalation),
        [false, false, false, false, false],
    );
}
