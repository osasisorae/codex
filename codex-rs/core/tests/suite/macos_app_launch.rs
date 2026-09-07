#![cfg(target_os = "macos")]

use anyhow::Result;
use codex_config::types::ApprovalsReviewer;
use codex_core::config::Constrained;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewDecision;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;

const ESCALATION_JUSTIFICATION: &str =
    "Launching a macOS application requires access to LaunchServices outside the Codex sandbox.";
const SANDBOX_RETRY_REASON: &str =
    "A macOS application launch was blocked before AppKit could crash; retry outside the sandbox?";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn app_bundle_launch_prompts_then_runs_outside_seatbelt() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let mut builder = test_codex().with_model("gpt-5.4").with_config(|config| {
        config.permissions.approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
        config
            .permissions
            .set_permission_profile(PermissionProfile::workspace_write())
            .expect("set workspace-write permissions");
        config.approvals_reviewer = ApprovalsReviewer::User;
    });
    let test = builder.build_with_auto_env(&server).await?;

    let executable = test
        .cwd
        .path()
        .join("Google Chrome.app/Contents/MacOS/Google Chrome");
    fs::create_dir_all(executable.parent().expect("fixture executable parent"))?;
    fs::write(&executable, "#!/bin/sh\nprintf launched > \"$1\"\n")?;
    let mut permissions = fs::metadata(&executable)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions)?;

    let output_path = test.home.path().join("app-launch-result.txt");
    let escaped_executable = executable.to_string_lossy().replace(' ', r"\ ");
    let command = format!("{escaped_executable} '{}'", output_path.display());
    let call_id = "macos-app-launch";
    let args = json!({"cmd": command, "yield_time_ms": 10_000});
    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-app-launch-1"),
            ev_function_call(call_id, "exec_command", &serde_json::to_string(&args)?),
            ev_completed("resp-app-launch-1"),
        ]),
    )
    .await;
    let results_mock = mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-app-launch-1", "done"),
            ev_completed("resp-app-launch-2"),
        ]),
    )
    .await;

    test.submit_text_turn("launch the fixture application")
        .await?;
    let event = wait_for_event(&test.codex, |event| {
        matches!(
            event,
            EventMsg::ExecApprovalRequest(_) | EventMsg::TurnComplete(_)
        )
    })
    .await;
    let EventMsg::ExecApprovalRequest(approval) = event else {
        panic!("expected macOS application launch approval before execution");
    };
    assert_eq!(approval.call_id, call_id);
    assert_eq!(approval.reason.as_deref(), Some(ESCALATION_JUSTIFICATION));
    assert!(!output_path.exists(), "application ran before approval");

    test.codex
        .submit(Op::ExecApproval {
            id: approval.effective_approval_id(),
            turn_id: None,
            decision: ReviewDecision::Approved,
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    assert_eq!(fs::read_to_string(&output_path)?, "launched");
    let output = results_mock
        .single_request()
        .function_call_output_text(call_id)
        .expect("application launch output");
    assert!(output.contains("Process exited with code 0"), "{output}");
    test.codex.shutdown_and_wait().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_app_bundle_launch_is_blocked_then_retried_after_approval() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let mut builder = test_codex().with_model("gpt-5.4").with_config(|config| {
        config.permissions.approval_policy = Constrained::allow_any(AskForApproval::OnRequest);
        config
            .permissions
            .set_permission_profile(PermissionProfile::workspace_write())
            .expect("set workspace-write permissions");
        config.approvals_reviewer = ApprovalsReviewer::User;
    });
    let test = builder.build_with_auto_env(&server).await?;

    let executable = test
        .cwd
        .path()
        .join("Nested Browser.app/Contents/MacOS/Nested Browser");
    fs::create_dir_all(executable.parent().expect("fixture executable parent"))?;
    fs::write(&executable, "#!/bin/sh\nprintf launched > \"$1\"\n")?;
    let mut permissions = fs::metadata(&executable)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions)?;

    let output_path = test.home.path().join("nested-app-launch-result.txt");
    let inner_command = format!("'{}' '{}'", executable.display(), output_path.display());
    // The visible command is a generic shell. Only its nested child resolves to
    // an application bundle, matching Node/Playwright-style indirect launches.
    let command = format!("/bin/sh -c \"{inner_command}\"");
    let call_id = "nested-macos-app-launch";
    let args = json!({"cmd": command, "yield_time_ms": 10_000});
    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-nested-app-launch-1"),
            ev_function_call(call_id, "exec_command", &serde_json::to_string(&args)?),
            ev_completed("resp-nested-app-launch-1"),
        ]),
    )
    .await;
    let results_mock = mount_sse_once(
        &server,
        sse(vec![
            ev_assistant_message("msg-nested-app-launch-1", "done"),
            ev_completed("resp-nested-app-launch-2"),
        ]),
    )
    .await;

    test.submit_text_turn("launch an application through a nested tool")
        .await?;
    let event = wait_for_event(&test.codex, |event| {
        matches!(
            event,
            EventMsg::ExecApprovalRequest(_) | EventMsg::TurnComplete(_)
        )
    })
    .await;
    let EventMsg::ExecApprovalRequest(approval) = event else {
        panic!("expected approval after Seatbelt blocked the nested application");
    };
    assert_eq!(approval.call_id, call_id);
    assert_eq!(approval.reason.as_deref(), Some(SANDBOX_RETRY_REASON));
    assert!(
        !output_path.exists(),
        "nested application ran before approval"
    );

    test.codex
        .submit(Op::ExecApproval {
            id: approval.effective_approval_id(),
            turn_id: None,
            decision: ReviewDecision::Approved,
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;

    assert_eq!(fs::read_to_string(&output_path)?, "launched");
    let output = results_mock
        .single_request()
        .function_call_output_text(call_id)
        .expect("nested application launch output");
    assert!(output.contains("Process exited with code 0"), "{output}");
    test.codex.shutdown_and_wait().await?;
    Ok(())
}
